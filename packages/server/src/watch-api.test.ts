import { HttpApiBuilder, HttpServer } from "@effect/platform";
import * as BunFileSystem from "@effect/platform-bun/BunFileSystem";
import { afterAll, describe, expect, test } from "bun:test";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { Layer } from "effect";
import { VIGIL_DAEMON_TOKEN_HEADER } from "@vigil/api";
import { makeVigilApiLayer } from "./index.ts";

interface TestRepo {
	readonly repoPath: string;
	readonly trackedFilePath: string;
}

interface WatchSubscribeResponseBody {
	readonly subscriptionId: string;
	readonly repoRoot: string;
	readonly version: number;
}

interface RepoChangedEventBody {
	readonly subscriptionId: string;
	readonly repoRoot: string;
	readonly version: number;
	readonly changedAt: string;
}

type WebHandler = ReturnType<typeof HttpApiBuilder.toWebHandler>["handler"];

function createSseReaderState(response: Response) {
	const reader = response.body?.getReader();
	if (!reader) {
		throw new Error("SSE response does not have a readable body.");
	}

	return {
		reader,
		buffer: "",
	};
}

type SseReaderState = ReturnType<typeof createSseReaderState>;
const sseReaderStates = new WeakMap<Response, SseReaderState>();

async function runCommand(
	command: ReadonlyArray<string>,
	options: {
		readonly cwd?: string;
	} = {},
): Promise<string> {
	const process = options.cwd
		? Bun.spawn([...command], {
				cwd: options.cwd,
				stdout: "pipe",
				stderr: "pipe",
			})
		: Bun.spawn([...command], {
				stdout: "pipe",
				stderr: "pipe",
			});

	const [exitCode, stdout, stderr] = await Promise.all([
		process.exited,
		process.stdout ? new Response(process.stdout).text() : Promise.resolve(""),
		process.stderr ? new Response(process.stderr).text() : Promise.resolve(""),
	]);

	if (exitCode !== 0) {
		throw new Error(
			`${command.join(" ")} failed with exit code ${exitCode}\n${stderr.trim() || stdout.trim()}`,
		);
	}

	return stdout;
}

async function createGitRepository(): Promise<TestRepo> {
	const repoPath = await mkdtemp(path.join(tmpdir(), "vigil-watch-api-"));
	const trackedFilePath = path.join(repoPath, "tracked.txt");

	await runCommand(["git", "init"], { cwd: repoPath });
	await runCommand(["git", "config", "user.email", "watch-tests@example.com"], {
		cwd: repoPath,
	});
	await runCommand(["git", "config", "user.name", "Watch Tests"], {
		cwd: repoPath,
	});
	await writeFile(trackedFilePath, "line 1\n", "utf8");
	await runCommand(["git", "add", "tracked.txt"], { cwd: repoPath });
	await runCommand(["git", "commit", "-m", "initial"], { cwd: repoPath });

	return { repoPath, trackedFilePath };
}

function makeTestHandler(daemonToken: string) {
	const layer = Layer.mergeAll(
		makeVigilApiLayer({
			host: "127.0.0.1",
			port: 4096,
			daemonToken,
		}).pipe(Layer.provide(BunFileSystem.layer)),
		HttpServer.layerContext,
	);

	return HttpApiBuilder.toWebHandler(layer);
}

async function request(
	handler: WebHandler,
	options: {
		readonly daemonToken: string;
		readonly method: "GET" | "POST";
		readonly pathname: string;
		readonly body?: unknown;
	},
): Promise<Response> {
	const headers = new Headers({
		[VIGIL_DAEMON_TOKEN_HEADER]: options.daemonToken,
	});
	if (options.body !== undefined) {
		headers.set("content-type", "application/json");
	}

	return handler(
		new Request(`http://localhost${options.pathname}`, {
			method: options.method,
			headers,
			body:
				options.body === undefined ? undefined : JSON.stringify(options.body),
		}),
	);
}

async function subscribeWatch(
	handler: WebHandler,
	daemonToken: string,
	payload: {
		readonly clientId: string;
		readonly repoPath: string;
	},
): Promise<WatchSubscribeResponseBody> {
	const response = await request(handler, {
		daemonToken,
		method: "POST",
		pathname: "/watch/subscribe",
		body: payload,
	});
	expect(response.status).toBe(200);
	return (await response.json()) as WatchSubscribeResponseBody;
}

async function readNextRepoChangedEvent(
	response: Response,
	timeoutMs = 6_000,
): Promise<RepoChangedEventBody> {
	let state = sseReaderStates.get(response);
	if (!state) {
		state = createSseReaderState(response);
		sseReaderStates.set(response, state);
	}
	const activeState = state;

	const decoder = new TextDecoder();
	const deadline = Date.now() + timeoutMs;

	while (Date.now() < deadline) {
		const remainingMs = Math.max(1, deadline - Date.now());
		const readResult = await Promise.race([
			activeState.reader.read(),
			new Promise<never>((_, reject) =>
				setTimeout(() => reject(new Error("Timed out waiting for SSE data.")), remainingMs),
			),
		]);

		if (readResult.done) {
			throw new Error("SSE stream closed before receiving a repo-changed event.");
		}

		activeState.buffer += decoder.decode(readResult.value, { stream: true });

		while (true) {
			const separatorIndex = activeState.buffer.indexOf("\n\n");
			if (separatorIndex === -1) {
				break;
			}

			const block = activeState.buffer.slice(0, separatorIndex);
			activeState.buffer = activeState.buffer.slice(separatorIndex + 2);

			if (!block.includes("event: repo-changed")) {
				continue;
			}

			const dataLine = block
				.split("\n")
				.find((line) => line.startsWith("data: "));
			if (!dataLine) {
				throw new Error(`repo-changed event missing data payload: ${block}`);
			}

			return JSON.parse(dataLine.slice("data: ".length)) as RepoChangedEventBody;
		}
	}

	throw new Error(`Timed out waiting for repo-changed SSE event after ${timeoutMs}ms.`);
}

describe("watch api", () => {
	const daemonToken = "watch-api-test-token";
	const repoPromise = createGitRepository();

	afterAll(async () => {
		const repo = await repoPromise;
		await rm(repo.repoPath, { recursive: true, force: true });
	});

	test("subscribe dedupes by client and repo, and unsubscribe endpoints map errors", async () => {
		const repo = await repoPromise;
		const { handler, dispose } = makeTestHandler(daemonToken);

		try {
			const first = await subscribeWatch(handler, daemonToken, {
				clientId: "watch-client-a",
				repoPath: repo.repoPath,
			});
			const second = await subscribeWatch(handler, daemonToken, {
				clientId: "watch-client-a",
				repoPath: repo.repoPath,
			});

			expect(first.subscriptionId.length).toBeGreaterThan(0);
			expect(first.repoRoot).toBe(second.repoRoot);
			expect(first.subscriptionId).toBe(second.subscriptionId);
			expect(first.version).toBe(second.version);

			const missingResponse = await request(handler, {
				daemonToken,
				method: "POST",
				pathname: "/watch/unsubscribe",
				body: {
					clientId: "watch-client-a",
					subscriptionId: "missing-subscription-id",
				},
			});
			expect(missingResponse.status).toBe(404);
			const missingBody = (await missingResponse.json()) as {
				readonly _tag: string;
			};
			expect(missingBody._tag).toBe("WatchSubscriptionNotFoundError");

			const unsubscribeResponse = await request(handler, {
				daemonToken,
				method: "POST",
				pathname: "/watch/unsubscribe",
				body: {
					clientId: "watch-client-a",
					subscriptionId: first.subscriptionId,
				},
			});
			expect(unsubscribeResponse.status).toBe(204);

			const unsubscribeAllResponse = await request(handler, {
				daemonToken,
				method: "POST",
				pathname: "/watch/unsubscribe-all",
				body: {
					clientId: "watch-client-a",
				},
			});
			expect(unsubscribeAllResponse.status).toBe(204);
		} finally {
			await dispose();
		}
	});

	test("events emits repo-changed when repository content changes", async () => {
		const repo = await repoPromise;
		const { handler, dispose } = makeTestHandler(daemonToken);

		try {
			const subscription = await subscribeWatch(handler, daemonToken, {
				clientId: "watch-client-b",
				repoPath: repo.repoPath,
			});

			const eventsResponse = await request(handler, {
				daemonToken,
				method: "GET",
				pathname: "/watch/events?clientId=watch-client-b",
			});
			expect(eventsResponse.status).toBe(200);
			expect(eventsResponse.headers.get("content-type")).toContain(
				"text/event-stream",
			);

			await Bun.sleep(400);

			const current = await readFile(repo.trackedFilePath, "utf8");
			await writeFile(repo.trackedFilePath, `${current}line 2\n`, "utf8");

			const event = await readNextRepoChangedEvent(eventsResponse);
			expect(event.subscriptionId).toBe(subscription.subscriptionId);
			expect(event.repoRoot).toBe(subscription.repoRoot);
			expect(event.version).toBeGreaterThan(subscription.version);
			expect(typeof event.changedAt).toBe("string");
			expect(Number.isNaN(Date.parse(event.changedAt))).toBe(false);

			const unsubscribeAllResponse = await request(handler, {
				daemonToken,
				method: "POST",
				pathname: "/watch/unsubscribe-all",
				body: {
					clientId: "watch-client-b",
				},
			});
			expect(unsubscribeAllResponse.status).toBe(204);
		} finally {
			await dispose();
		}
	});

	test("events emits again when editing an already-modified file", async () => {
		const repo = await repoPromise;
		const { handler, dispose } = makeTestHandler(daemonToken);

		try {
			const subscription = await subscribeWatch(handler, daemonToken, {
				clientId: "watch-client-c",
				repoPath: repo.repoPath,
			});

			const eventsResponse = await request(handler, {
				daemonToken,
				method: "GET",
				pathname: "/watch/events?clientId=watch-client-c",
			});
			expect(eventsResponse.status).toBe(200);

			await Bun.sleep(400);

			const base = await readFile(repo.trackedFilePath, "utf8");
			const firstContent = `${base}content-change-a-${Date.now()}\n`;
			await writeFile(repo.trackedFilePath, firstContent, "utf8");
			const firstEvent = await readNextRepoChangedEvent(eventsResponse);
			expect(firstEvent.subscriptionId).toBe(subscription.subscriptionId);
			expect(firstEvent.version).toBeGreaterThan(subscription.version);

			const secondContent = `${firstContent}content-change-b-${Date.now()}\n`;
			await writeFile(repo.trackedFilePath, secondContent, "utf8");
			const secondEvent = await readNextRepoChangedEvent(eventsResponse);
			expect(secondEvent.subscriptionId).toBe(subscription.subscriptionId);
			expect(secondEvent.version).toBeGreaterThan(firstEvent.version);

			const unsubscribeAllResponse = await request(handler, {
				daemonToken,
				method: "POST",
				pathname: "/watch/unsubscribe-all",
				body: {
					clientId: "watch-client-c",
				},
			});
			expect(unsubscribeAllResponse.status).toBe(204);
		} finally {
			await dispose();
		}
	});
});
