import * as FileSystem from "@effect/platform/FileSystem";
import path from "node:path";
import {
	Context,
	Data,
	Effect,
	Fiber,
	Layer,
	Option,
	pipe,
	PubSub,
	Queue,
	Ref,
	Stream,
} from "effect";

const REPO_WATCH_DEBOUNCE_MS = 250;
const REPO_WATCH_SAFETY_POLL_MS = 30_000;

interface RepoWatcherState {
	readonly version: number;
	readonly snapshot: string;
}

interface ActiveRepoWatcher {
	readonly repoRoot: string;
	readonly stateRef: Ref.Ref<RepoWatcherState>;
	readonly fiber: Fiber.RuntimeFiber<void, never>;
	readonly refCount: number;
}

export interface RepoWatcherLease {
	readonly repoRoot: string;
	readonly version: number;
}

export interface RepoWatcherEvent {
	readonly repoRoot: string;
	readonly version: number;
	readonly changedAt: Date;
}

export class RepoWatcherResolveError extends Data.TaggedError(
	"RepoWatcherResolveError",
)<{
	readonly repoPath: string;
	readonly message: string;
	readonly cause?: unknown;
}> {}

export class RepoWatcherGitError extends Data.TaggedError(
	"RepoWatcherGitError",
)<{
	readonly repoRoot: string;
	readonly args: ReadonlyArray<string>;
	readonly message: string;
	readonly stdout: string;
	readonly stderr: string;
	readonly cause?: unknown;
}> {}

export type RepoWatcherRetainError =
	| RepoWatcherResolveError
	| RepoWatcherGitError;

function formatFsWatchEvent(event: unknown): string {
	if (typeof event === "string") {
		return event;
	}

	if (typeof event === "object" && event !== null) {
		const record = event as Record<string, unknown>;
		const kind =
			(typeof record._tag === "string" && record._tag) ||
			(typeof record.eventType === "string" && record.eventType) ||
			"event";
		const watchPath =
			(typeof record.path === "string" && record.path) ||
			(typeof record.filePath === "string" && record.filePath);
		if (watchPath) {
			return `${kind} ${watchPath}`;
		}
		try {
			return JSON.stringify(record);
		} catch {
			return kind;
		}
	}

	return String(event);
}

function shouldIgnoreFsEventPath(watchPath: string): boolean {
	const normalizedPath = watchPath.replaceAll("\\", "/").replace(/^\.\//, "");

	return (
		normalizedPath === ".git" ||
		normalizedPath.startsWith(".git/") ||
		normalizedPath.endsWith("/.git") ||
		normalizedPath.includes("/.git/")
	);
}

const runGitInRepo = Effect.fn("RepoWatcher.runGitInRepo")(function* (
	repoRoot: string,
	args: ReadonlyArray<string>,
	fallbackMessage: string,
) {
	const command = ["-C", repoRoot, ...args] as const;
	const result = yield* Effect.tryPromise({
		try: async () => {
			const process = Bun.spawn({
				cmd: ["git", ...command],
				stdout: "pipe",
				stderr: "pipe",
			});
			const [exitCode, stdout, stderr] = await Promise.all([
				process.exited,
				process.stdout
					? new Response(process.stdout).text()
					: Promise.resolve(""),
				process.stderr
					? new Response(process.stderr).text()
					: Promise.resolve(""),
			]);
			return { exitCode, stdout, stderr };
		},
		catch: (cause) =>
			new RepoWatcherGitError({
				repoRoot,
				args: [...command],
				message: fallbackMessage,
				stdout: "",
				stderr: "",
				cause,
			}),
	});

	if (result.exitCode !== 0) {
		return yield* new RepoWatcherGitError({
			repoRoot,
			args: [...command],
			message: result.stderr.trim() || result.stdout.trim() || fallbackMessage,
			stdout: result.stdout,
			stderr: result.stderr,
		});
	}

	return {
		stdout: result.stdout,
		stderr: result.stderr,
	};
});

const computeRepoSnapshot = Effect.fn("RepoWatcher.computeRepoSnapshot")(
	function* (repoRoot: string) {
		const statusResult = yield* runGitInRepo(
			repoRoot,
			["status", "--porcelain=v1", "-z", "--untracked-files=all"],
			`Unable to compute repository snapshot for ${repoRoot}.`,
		);
		const workingTreeDiff = yield* runGitInRepo(
			repoRoot,
			["diff", "--no-color", "--no-ext-diff", "--binary"],
			`Unable to compute working tree diff snapshot for ${repoRoot}.`,
		);
		const stagedDiff = yield* runGitInRepo(
			repoRoot,
			["diff", "--no-color", "--no-ext-diff", "--binary", "--cached"],
			`Unable to compute staged diff snapshot for ${repoRoot}.`,
		);

		return [
			statusResult.stdout,
			"\n--working-tree-diff--\n",
			workingTreeDiff.stdout,
			"\n--staged-diff--\n",
			stagedDiff.stdout,
		].join("");
	},
);

const resolveRepoRoot = Effect.fn("RepoWatcher.resolveRepoRoot")(function* (
	fs: FileSystem.FileSystem,
	repoPath: string,
) {
	const candidatePath = path.resolve(repoPath);
	const result = yield* pipe(
		runGitInRepo(
			candidatePath,
			["rev-parse", "--show-toplevel"],
			`Unable to resolve repository root for ${candidatePath}.`,
		),
		Effect.mapError(
			(cause) =>
				new RepoWatcherResolveError({
					repoPath: candidatePath,
					message: cause.message,
					cause,
				}),
		),
	);

	const repoRoot = result.stdout.trim();
	if (repoRoot.length === 0) {
		return yield* new RepoWatcherResolveError({
			repoPath: candidatePath,
			message: "Git returned an empty repository root.",
		});
	}

	return yield* pipe(
		fs.realPath(repoRoot),
		Effect.catchTag("SystemError", (cause) =>
			Effect.fail(
				new RepoWatcherResolveError({
					repoPath: candidatePath,
					message: `Unable to canonicalize repository root: ${cause.message}`,
					cause,
				}),
			),
		),
		Effect.catchTag("BadArgument", (cause) =>
			Effect.fail(
				new RepoWatcherResolveError({
					repoPath: candidatePath,
					message: `Unable to canonicalize repository root: ${cause.message}`,
					cause,
				}),
			),
		),
	);
});

function makeRepoWatcherLoop(options: {
	readonly fs: FileSystem.FileSystem;
	readonly repoRoot: string;
	readonly stateRef: Ref.Ref<RepoWatcherState>;
	readonly eventPubSub: PubSub.PubSub<RepoWatcherEvent>;
}): Effect.Effect<void, never> {
	return Effect.gen(function* () {
		const triggerQueue = yield* Queue.sliding<void>(1);
		const triggerRefresh = Queue.offer(triggerQueue, undefined).pipe(
			Effect.asVoid,
		);
		yield* Effect.logInfo(`[repo-watcher] watching ${options.repoRoot}`);

		const fsWatchFiber = yield* options.fs
			.watch(options.repoRoot, { recursive: true })
			.pipe(
				Stream.filter((event) => !shouldIgnoreFsEventPath(event.path)),
				Stream.runForEach((event) =>
					triggerRefresh.pipe(
						Effect.zipRight(
							Effect.logInfo(
								`[repo-watcher] fs event for ${options.repoRoot}: ${formatFsWatchEvent(event)}`,
							),
						),
					),
				),
				Effect.catchAll((error) =>
					Effect.logWarning(
						`[repo-watcher] filesystem watch stream failed for ${options.repoRoot}: ${error.message}`,
					),
				),
				Effect.forkDaemon,
			);

		const safetyPollFiber = yield* pipe(
			Effect.forever(
				Effect.sleep(`${REPO_WATCH_SAFETY_POLL_MS} millis`).pipe(
					Effect.zipRight(triggerRefresh),
				),
			),
			Effect.forkDaemon,
		);

		yield* triggerRefresh;

		yield* Effect.forever(
			Effect.gen(function* () {
				yield* Queue.take(triggerQueue);
				yield* Effect.sleep(`${REPO_WATCH_DEBOUNCE_MS} millis`);
				yield* Queue.takeAll(triggerQueue);

				const maybeNextSnapshot = yield* pipe(
					computeRepoSnapshot(options.repoRoot),
					Effect.map(Option.some),
					Effect.catchAll((error) =>
						Effect.logWarning(
							`[repo-watcher] snapshot refresh failed for ${options.repoRoot}: ${error.message}`,
						).pipe(Effect.as(Option.none<string>())),
					),
				);

				if (Option.isNone(maybeNextSnapshot)) {
					return;
				}

				const nextSnapshot = maybeNextSnapshot.value;
				const currentState = yield* Ref.get(options.stateRef);
				if (currentState.snapshot === nextSnapshot) {
					return;
				}

				const nextVersion = currentState.version + 1;
				yield* Ref.set(options.stateRef, {
					version: nextVersion,
					snapshot: nextSnapshot,
				});
				yield* Effect.logInfo(
					`[repo-watcher] snapshot changed repoRoot=${options.repoRoot} version=${nextVersion}`,
				);

				yield* PubSub.publish(options.eventPubSub, {
					repoRoot: options.repoRoot,
					version: nextVersion,
					changedAt: new Date(),
				});
			}),
		).pipe(
			Effect.ensuring(
				Effect.all(
					[
						Fiber.interrupt(fsWatchFiber),
						Fiber.interrupt(safetyPollFiber),
						Queue.shutdown(triggerQueue),
					],
					{ discard: true },
				).pipe(
					Effect.zipRight(
						Effect.logInfo(
							`[repo-watcher] stopped watching ${options.repoRoot}`,
						),
					),
				),
			),
		);
	});
}

export class RepoWatcher extends Context.Tag("@vigil/server/RepoWatcher")<
	RepoWatcher,
	{
		readonly retain: (
			repoPath: string,
		) => Effect.Effect<RepoWatcherLease, RepoWatcherRetainError>;
		readonly release: (repoRoot: string) => Effect.Effect<void>;
		readonly currentVersion: (
			repoRoot: string,
		) => Effect.Effect<Option.Option<number>>;
		readonly events: () => Stream.Stream<RepoWatcherEvent>;
	}
>() {
	static readonly layer = Layer.scoped(
		RepoWatcher,
		Effect.gen(function* () {
			const fs = yield* FileSystem.FileSystem;
			const eventPubSub = yield* PubSub.unbounded<RepoWatcherEvent>();
			const lock = yield* Effect.makeSemaphore(1);
			const activeWatchers = new Map<string, ActiveRepoWatcher>();

			const withLock = <A, E, R>(effect: Effect.Effect<A, E, R>) =>
				lock.withPermits(1)(effect);

			yield* Effect.addFinalizer(() =>
				Effect.gen(function* () {
					const fibers = yield* withLock(
						Effect.sync(() =>
							Array.from(activeWatchers.values(), (watcher) => watcher.fiber),
						),
					);
					yield* Effect.logInfo(
						`[repo-watcher] shutting down ${fibers.length} watcher(s)`,
					);

					yield* Effect.all(
						fibers.map((fiber) => Fiber.interrupt(fiber)),
						{ discard: true },
					);
					yield* PubSub.shutdown(eventPubSub);
				}),
			);

			const retain = Effect.fn("RepoWatcher.retain")(function* (
				repoPath: string,
			) {
				const repoRoot = yield* resolveRepoRoot(fs, repoPath);
				yield* Effect.logInfo(
					`[repo-watcher] retain requested repoPath=${repoPath} repoRoot=${repoRoot}`,
				);

				return yield* withLock(
					Effect.gen(function* () {
						const existing = activeWatchers.get(repoRoot);
						if (existing) {
							const currentState = yield* Ref.get(existing.stateRef);
							const nextRefCount = existing.refCount + 1;
							activeWatchers.set(repoRoot, {
								...existing,
								refCount: nextRefCount,
							});
							yield* Effect.logInfo(
								`[repo-watcher] reused watcher repoRoot=${repoRoot} refCount=${nextRefCount} version=${currentState.version}`,
							);
							return {
								repoRoot,
								version: currentState.version,
							};
						}

						const initialSnapshot = yield* computeRepoSnapshot(repoRoot);
						const stateRef = yield* Ref.make<RepoWatcherState>({
							version: 0,
							snapshot: initialSnapshot,
						});

						const fiber = yield* makeRepoWatcherLoop({
							fs,
							repoRoot,
							stateRef,
							eventPubSub,
						}).pipe(Effect.forkDaemon);

						activeWatchers.set(repoRoot, {
							repoRoot,
							refCount: 1,
							stateRef,
							fiber,
						});
						yield* Effect.logInfo(
							`[repo-watcher] created watcher repoRoot=${repoRoot} refCount=1 version=0`,
						);

						return {
							repoRoot,
							version: 0,
						};
					}),
				);
			});

			const release = Effect.fn("RepoWatcher.release")(function* (
				repoRoot: string,
			) {
				const releaseResult = yield* withLock(
					Effect.sync(() => {
						const current = activeWatchers.get(repoRoot);
						if (!current) {
							return {
								_tag: "missing" as const,
							};
						}

						if (current.refCount > 1) {
							const nextRefCount = current.refCount - 1;
							activeWatchers.set(repoRoot, {
								...current,
								refCount: nextRefCount,
							});
							return {
								_tag: "decremented" as const,
								refCount: nextRefCount,
							};
						}

						activeWatchers.delete(repoRoot);
						return {
							_tag: "stopped" as const,
							fiber: current.fiber,
						};
					}),
				);

				if (releaseResult._tag === "missing") {
					yield* Effect.logInfo(
						`[repo-watcher] release ignored repoRoot=${repoRoot} (no active watcher)`,
					);
					return;
				}

				if (releaseResult._tag === "decremented") {
					yield* Effect.logInfo(
						`[repo-watcher] decremented watcher repoRoot=${repoRoot} refCount=${releaseResult.refCount}`,
					);
					return;
				}

				yield* Effect.logInfo(
					`[repo-watcher] stopping watcher repoRoot=${repoRoot}`,
				);
				yield* Fiber.interrupt(releaseResult.fiber);
			});

			const currentVersion = Effect.fn("RepoWatcher.currentVersion")(function* (
				repoRoot: string,
			) {
				const maybeStateRef = yield* withLock(
					Effect.sync(() =>
						Option.fromNullable(activeWatchers.get(repoRoot)?.stateRef),
					),
				);
				if (Option.isNone(maybeStateRef)) {
					return Option.none<number>();
				}

				const state = yield* Ref.get(maybeStateRef.value);
				return Option.some(state.version);
			});

			return RepoWatcher.of({
				retain,
				release,
				currentVersion,
				events: () => Stream.fromPubSub(eventPubSub),
			});
		}),
	);
}
