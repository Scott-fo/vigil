import { Context, Data, Effect, Fiber, Layer, PubSub, Ref, Stream } from "effect";
import {
	RepoWatcher,
	type RepoWatcherEvent,
	type RepoWatcherRetainError,
} from "./repo-watcher.ts";

interface ClientSubscriptionState {
	readonly subscriptionId: string;
	readonly repoRoot: string;
	readonly version: number;
}

interface ClientState {
	readonly pubsub: PubSub.PubSub<RepoSubscriptionEvent>;
	readonly subscriptions: Map<string, ClientSubscriptionState>;
	readonly repoIndex: Map<string, string>;
}

export interface RepoSubscriptionLease {
	readonly clientId: string;
	readonly subscriptionId: string;
	readonly repoRoot: string;
	readonly version: number;
}

export interface RepoSubscriptionEvent {
	readonly clientId: string;
	readonly subscriptionId: string;
	readonly repoRoot: string;
	readonly version: number;
	readonly changedAt: Date;
}

export class RepoSubscriptionClientIdError extends Data.TaggedError(
	"RepoSubscriptionClientIdError",
)<{
	readonly message: string;
}> {}

export class RepoSubscriptionNotFoundError extends Data.TaggedError(
	"RepoSubscriptionNotFoundError",
)<{
	readonly clientId: string;
	readonly subscriptionId: string;
	readonly message: string;
}> {}

export type RepoSubscriptionSubscribeError =
	| RepoSubscriptionClientIdError
	| RepoWatcherRetainError;

export type RepoSubscriptionUnsubscribeError =
	| RepoSubscriptionClientIdError
	| RepoSubscriptionNotFoundError;

function isBlank(value: string): boolean {
	return value.trim().length === 0;
}

function createSubscriptionId(): string {
	return crypto.randomUUID();
}

function toRepoSubscriptionEvent(
	clientId: string,
	subscription: ClientSubscriptionState,
	event: RepoWatcherEvent,
): RepoSubscriptionEvent {
	return {
		clientId,
		subscriptionId: subscription.subscriptionId,
		repoRoot: subscription.repoRoot,
		version: event.version,
		changedAt: event.changedAt,
	};
}

export class RepoSubscription extends Context.Tag(
	"@vigil/server/RepoSubscription",
)<
	RepoSubscription,
	{
		readonly subscribe: (
			clientId: string,
			repoPath: string,
		) => Effect.Effect<RepoSubscriptionLease, RepoSubscriptionSubscribeError>;
		readonly unsubscribe: (
			clientId: string,
			subscriptionId: string,
		) => Effect.Effect<void, RepoSubscriptionUnsubscribeError>;
		readonly unsubscribeAll: (clientId: string) => Effect.Effect<void>;
		readonly events: (
			clientId: string,
		) => Effect.Effect<
			Stream.Stream<RepoSubscriptionEvent>,
			RepoSubscriptionClientIdError
		>;
	}
>() {
	static readonly layer = Layer.scoped(
		RepoSubscription,
		Effect.gen(function* () {
			const watcher = yield* RepoWatcher;
			const lock = yield* Effect.makeSemaphore(1);
			const clients = new Map<string, ClientState>();

			const withLock = <A, E, R>(effect: Effect.Effect<A, E, R>) =>
				lock.withPermits(1)(effect);

			const routeWatcherEvent = Effect.fn("RepoSubscription.routeWatcherEvent")(
				function* (event: RepoWatcherEvent) {
					const deliveries = yield* withLock(
						Effect.sync(() => {
							const result: Array<{
								readonly pubsub: PubSub.PubSub<RepoSubscriptionEvent>;
								readonly event: RepoSubscriptionEvent;
							}> = [];

							for (const [clientId, state] of clients) {
								const subscriptionId = state.repoIndex.get(event.repoRoot);
								if (!subscriptionId) {
									continue;
								}

								const subscription = state.subscriptions.get(subscriptionId);
								if (!subscription) {
									state.repoIndex.delete(event.repoRoot);
									continue;
								}

								const updatedSubscription: ClientSubscriptionState = {
									...subscription,
									version: event.version,
								};
								state.subscriptions.set(subscriptionId, updatedSubscription);

								result.push({
									pubsub: state.pubsub,
									event: toRepoSubscriptionEvent(
										clientId,
										updatedSubscription,
										event,
									),
								});
							}

							return result;
						}),
					);

					if (deliveries.length === 0) {
						return;
					}

					yield* Effect.all(
						deliveries.map((delivery) =>
							PubSub.publish(delivery.pubsub, delivery.event).pipe(
								Effect.asVoid,
							),
						),
						{ discard: true },
					);
				},
			);

			const routerFiber = yield* watcher
				.events()
				.pipe(Stream.runForEach((event) => routeWatcherEvent(event)), Effect.forkDaemon);

			yield* Effect.addFinalizer(() =>
				Effect.gen(function* () {
					yield* Fiber.interrupt(routerFiber);

					const cleanupPlan = yield* withLock(
						Effect.sync(() => {
							const pubsubs: Array<PubSub.PubSub<RepoSubscriptionEvent>> = [];
							const repoRoots: Array<string> = [];

							for (const [, state] of clients) {
								pubsubs.push(state.pubsub);
								for (const [, subscription] of state.subscriptions) {
									repoRoots.push(subscription.repoRoot);
								}
							}

							clients.clear();
							return { pubsubs, repoRoots };
						}),
					);

					yield* Effect.all(
						cleanupPlan.repoRoots.map((repoRoot) => watcher.release(repoRoot)),
						{ discard: true },
					);
					yield* Effect.all(
						cleanupPlan.pubsubs.map((pubsub) => PubSub.shutdown(pubsub)),
						{ discard: true },
					);
				}),
			);

			const subscribe = Effect.fn("RepoSubscription.subscribe")(function* (
				clientId: string,
				repoPath: string,
			) {
				const normalizedClientId = clientId.trim();
				if (isBlank(normalizedClientId)) {
					return yield* new RepoSubscriptionClientIdError({
						message: "clientId must not be empty.",
					});
				}

				const watcherLease = yield* watcher.retain(repoPath);
				const releaseWatcherLease = watcher.release(watcherLease.repoRoot);

				return yield* Effect.uninterruptibleMask((restore) =>
					restore(
						withLock(
							Effect.gen(function* () {
								const existingClient = clients.get(normalizedClientId);
								const client =
									existingClient ??
									({
										pubsub: yield* PubSub.unbounded<RepoSubscriptionEvent>(),
										subscriptions: new Map<string, ClientSubscriptionState>(),
										repoIndex: new Map<string, string>(),
									} satisfies ClientState);

								if (!existingClient) {
									clients.set(normalizedClientId, client);
								}

								const existingSubscriptionId = client.repoIndex.get(
									watcherLease.repoRoot,
								);
								if (existingSubscriptionId) {
									const existingSubscription = client.subscriptions.get(
										existingSubscriptionId,
									);
									if (existingSubscription) {
										yield* releaseWatcherLease;
										return {
											clientId: normalizedClientId,
											subscriptionId: existingSubscription.subscriptionId,
											repoRoot: existingSubscription.repoRoot,
											version: existingSubscription.version,
										};
									}
									client.repoIndex.delete(watcherLease.repoRoot);
								}

								const nextSubscription: ClientSubscriptionState = {
									subscriptionId: createSubscriptionId(),
									repoRoot: watcherLease.repoRoot,
									version: watcherLease.version,
								};
								client.repoIndex.set(
									nextSubscription.repoRoot,
									nextSubscription.subscriptionId,
								);
								client.subscriptions.set(
									nextSubscription.subscriptionId,
									nextSubscription,
								);

								return {
									clientId: normalizedClientId,
									subscriptionId: nextSubscription.subscriptionId,
									repoRoot: nextSubscription.repoRoot,
									version: nextSubscription.version,
								};
							}),
						),
					).pipe(Effect.catchAllCause((cause) => releaseWatcherLease.pipe(Effect.zipRight(Effect.failCause(cause))))),
				);
			});

			const unsubscribe = Effect.fn("RepoSubscription.unsubscribe")(function* (
				clientId: string,
				subscriptionId: string,
			) {
				const normalizedClientId = clientId.trim();
				if (isBlank(normalizedClientId)) {
					return yield* new RepoSubscriptionClientIdError({
						message: "clientId must not be empty.",
					});
				}

				const normalizedSubscriptionId = subscriptionId.trim();
				const removed = yield* withLock(
					Effect.sync(() => {
						const client = clients.get(normalizedClientId);
						if (!client) {
							return null;
						}

						const subscription = client.subscriptions.get(normalizedSubscriptionId);
						if (!subscription) {
							return null;
						}

						client.subscriptions.delete(normalizedSubscriptionId);
						client.repoIndex.delete(subscription.repoRoot);

						const clientPubSub =
							client.subscriptions.size === 0 ? client.pubsub : null;
						if (clientPubSub) {
							clients.delete(normalizedClientId);
						}

						return {
							repoRoot: subscription.repoRoot,
							clientPubSub,
						};
					}),
				);

				if (!removed) {
					return yield* new RepoSubscriptionNotFoundError({
						clientId: normalizedClientId,
						subscriptionId: normalizedSubscriptionId,
						message: "Subscription not found for this client.",
					});
				}

				yield* watcher.release(removed.repoRoot);
				if (removed.clientPubSub) {
					yield* PubSub.shutdown(removed.clientPubSub);
				}
			});

			const unsubscribeAll = Effect.fn("RepoSubscription.unsubscribeAll")(function* (
				clientId: string,
			) {
				const normalizedClientId = clientId.trim();
				if (isBlank(normalizedClientId)) {
					return;
				}

				const removed = yield* withLock(
					Effect.sync(() => {
						const client = clients.get(normalizedClientId);
						if (!client) {
							return null;
						}

						clients.delete(normalizedClientId);
						return {
							repoRoots: Array.from(
								client.subscriptions.values(),
								(subscription) => subscription.repoRoot,
							),
							pubsub: client.pubsub,
						};
					}),
				);

				if (!removed) {
					return;
				}

				yield* Effect.all(
					removed.repoRoots.map((repoRoot) => watcher.release(repoRoot)),
					{ discard: true },
				);
				yield* PubSub.shutdown(removed.pubsub);
			});

			const events = Effect.fn("RepoSubscription.events")(function* (
				clientId: string,
			) {
				const normalizedClientId = clientId.trim();
				if (isBlank(normalizedClientId)) {
					return yield* new RepoSubscriptionClientIdError({
						message: "clientId must not be empty.",
					});
				}

				const pubsub = yield* withLock(
					Effect.gen(function* () {
						const existingClient = clients.get(normalizedClientId);
						if (existingClient) {
							return existingClient.pubsub;
						}

						const nextPubSub = yield* PubSub.unbounded<RepoSubscriptionEvent>();
						clients.set(normalizedClientId, {
							pubsub: nextPubSub,
							subscriptions: new Map<string, ClientSubscriptionState>(),
							repoIndex: new Map<string, string>(),
						});
						return nextPubSub;
					}),
				);

				return Stream.fromPubSub(pubsub);
			});

			return RepoSubscription.of({
				subscribe,
				unsubscribe,
				unsubscribeAll,
				events,
			});
		}),
	);
}
