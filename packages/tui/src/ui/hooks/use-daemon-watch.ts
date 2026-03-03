import { VIGIL_DAEMON_TOKEN_HEADER } from "@vigil/api";
import { useEffect, useMemo } from "react";
import {
	buildVigilDaemonBaseUrl,
	type VigilDaemonApiCall,
	type VigilDaemonConnection,
} from "#daemon/client.ts";
import {
	appendSseChunk,
	drainSseBlocks,
	parseRepoChangedEventBlock,
} from "#daemon/watch-events.ts";

interface UseDaemonWatchOptions {
	readonly daemonApiCall: VigilDaemonApiCall;
	readonly daemonConnection: VigilDaemonConnection;
	readonly repoPath: string;
	readonly enabled: boolean;
	readonly onRefreshInstruction: () => Promise<void>;
	readonly reconnectDelayMs?: number;
}

function isAbortError(error: unknown): boolean {
	return error instanceof Error && error.name === "AbortError";
}

async function consumeWatchEventStream(
	response: Response,
	onRefreshInstruction: () => Promise<void>,
): Promise<void> {
	const body = response.body;
	if (!body) {
		throw new Error("Watch events response body is missing.");
	}

	const reader = body.getReader();
	const decoder = new TextDecoder();
	let buffer = "";

	while (true) {
		const { done, value } = await reader.read();
		if (done) {
			return;
		}

		buffer = appendSseChunk(buffer, decoder.decode(value, { stream: true }));
		const drained = drainSseBlocks(buffer);
		buffer = drained.remaining;

		for (const block of drained.blocks) {
			const parsedEvent = parseRepoChangedEventBlock(block);
			if (!parsedEvent) {
				continue;
			}

			try {
				await onRefreshInstruction();
			} catch {}
		}
	}
}

export function useDaemonWatch(options: UseDaemonWatchOptions) {
	const {
		daemonApiCall,
		daemonConnection,
		repoPath,
		enabled,
		onRefreshInstruction,
	} = options;
	const reconnectDelayMs = options.reconnectDelayMs ?? 1_500;
	const clientId = useMemo(() => `vigil-tui-${crypto.randomUUID()}`, []);

	useEffect(() => {
		if (!enabled) {
			return;
		}

		let cancelled = false;
		let streamController: AbortController | null = null;
		let didUnsubscribe = false;

		const unsubscribeAll = async () => {
			if (didUnsubscribe) {
				return;
			}
			didUnsubscribe = true;
			try {
				await daemonApiCall((client) =>
					client.watch.unsubscribeAll({
						payload: {
							clientId,
						},
					}),
				);
			} catch {}
		};

		const run = async () => {
			while (!cancelled) {
				try {
					await daemonApiCall((client) =>
						client.watch.subscribe({
							payload: {
								clientId,
								repoPath,
							},
						}),
					);
					try {
						await onRefreshInstruction();
					} catch {}

					streamController = new AbortController();
					const response = await fetch(
						`${buildVigilDaemonBaseUrl(daemonConnection)}/watch/events?clientId=${encodeURIComponent(clientId)}`,
						{
							headers: {
								[VIGIL_DAEMON_TOKEN_HEADER]: daemonConnection.token,
							},
							signal: streamController.signal,
						},
					);

					if (!response.ok) {
						throw new Error(
							`Watch events stream failed with status ${response.status}.`,
						);
					}

					await consumeWatchEventStream(response, onRefreshInstruction);
				} catch (error) {
					if (cancelled || isAbortError(error)) {
						break;
					}

					await Bun.sleep(reconnectDelayMs);
				}
			}

			await unsubscribeAll();
		};

		void run();

		return () => {
			cancelled = true;
			streamController?.abort();
			void unsubscribeAll();
		};
	}, [
		clientId,
		daemonApiCall,
		daemonConnection,
		enabled,
		onRefreshInstruction,
		reconnectDelayMs,
		repoPath,
	]);
}
