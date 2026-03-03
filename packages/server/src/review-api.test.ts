import { HttpApiBuilder, HttpServer } from "@effect/platform";
import * as BunFileSystem from "@effect/platform-bun/BunFileSystem";
import { afterEach, describe, expect, test } from "bun:test";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { Layer } from "effect";
import { VIGIL_DAEMON_TOKEN_HEADER } from "@vigil/api";
import { makeVigilApiLayer } from "./index.ts";

type WebHandler = ReturnType<typeof HttpApiBuilder.toWebHandler>["handler"];

async function makeTestHandler(daemonToken: string) {
	const dataHome = await mkdtemp(path.join(tmpdir(), "vigil-review-api-data-"));
	process.env.XDG_DATA_HOME = dataHome;

	const layer = Layer.mergeAll(
		makeVigilApiLayer({
			host: "127.0.0.1",
			port: 4098,
			daemonToken,
		}).pipe(Layer.provide(BunFileSystem.layer)),
		HttpServer.layerContext,
	);

	const runtime = HttpApiBuilder.toWebHandler(layer);

	return {
		handler: runtime.handler,
		dispose: runtime.dispose,
		dataHome,
	};
}

async function request(
	handler: WebHandler,
	options: {
		readonly daemonToken: string;
		readonly method: "POST";
		readonly pathname: string;
		readonly body: unknown;
	},
): Promise<Response> {
	return handler(
		new Request(`http://localhost${options.pathname}`, {
			method: options.method,
			headers: {
				[VIGIL_DAEMON_TOKEN_HEADER]: options.daemonToken,
				"content-type": "application/json",
			},
			body: JSON.stringify(options.body),
		}),
	);
}

describe("review api", () => {
	const daemonToken = "review-api-test-token";
	const cleanupPaths: Array<string> = [];

	afterEach(async () => {
		await Promise.all(
			cleanupPaths.splice(0).map((directoryPath) =>
				rm(directoryPath, { recursive: true, force: true }),
			),
		);
	});

	test("create overall thread and list returns it", async () => {
		const { handler, dispose, dataHome } = await makeTestHandler(daemonToken);
		cleanupPaths.push(dataHome);

		try {
			const scope = {
				repoRoot: "/tmp/review-api-repo",
				mode: "working-tree" as const,
				sourceRef: null,
				destinationRef: null,
				scopeKey: "working-tree:main",
			};

			const createResponse = await request(handler, {
				daemonToken,
				method: "POST",
				pathname: "/review/threads/overall",
				body: {
					scope,
					body: "Overall note",
					author: null,
					threadId: null,
					commentId: null,
				},
			});
			expect(createResponse.status).toBe(200);
			const created = (await createResponse.json()) as {
				readonly thread: { readonly id: string };
				readonly comments: ReadonlyArray<{ readonly body: string }>;
				readonly isStale: boolean;
			};
			expect(created.comments).toHaveLength(1);
			expect(created.comments[0]?.body).toBe("Overall note");
			expect(created.isStale).toBe(false);

			const listResponse = await request(handler, {
				daemonToken,
				method: "POST",
				pathname: "/review/threads/list",
				body: {
					scope,
					filePath: null,
					includeResolved: false,
					includeStale: false,
					activeAnchors: null,
				},
			});
			expect(listResponse.status).toBe(200);

			const listed = (await listResponse.json()) as ReadonlyArray<{
				readonly thread: { readonly id: string };
				readonly isStale: boolean;
			}>;
			expect(listed).toHaveLength(1);
			expect(listed[0]?.thread.id).toBe(created.thread.id);
			expect(listed[0]?.isStale).toBe(false);
		} finally {
			await dispose();
		}
	});

	test("stale line thread is hidden by default and shown when includeStale is true", async () => {
		const { handler, dispose, dataHome } = await makeTestHandler(daemonToken);
		cleanupPaths.push(dataHome);

		try {
			const scope = {
				repoRoot: "/tmp/review-api-repo-2",
				mode: "working-tree" as const,
				sourceRef: null,
				destinationRef: null,
				scopeKey: "working-tree:feature/a",
			};

			const createResponse = await request(handler, {
				daemonToken,
				method: "POST",
				pathname: "/review/threads/line",
				body: {
					scope,
					anchor: {
						anchorType: "line",
						filePath: "src/file.ts",
						lineSide: "new",
						lineNumber: 42,
						hunkHeader: null,
						lineContentHash: null,
					},
					body: "Line note",
					author: null,
					threadId: null,
					commentId: null,
				},
			});
			expect(createResponse.status).toBe(200);

			const hiddenResponse = await request(handler, {
				daemonToken,
				method: "POST",
				pathname: "/review/threads/list",
				body: {
					scope,
					filePath: null,
					includeResolved: false,
					includeStale: false,
					activeAnchors: [
						{
							anchorType: "line",
							filePath: "src/file.ts",
							lineSide: "new",
							lineNumber: 7,
							hunkHeader: null,
							lineContentHash: null,
						},
					],
				},
			});
			expect(hiddenResponse.status).toBe(200);
			const hidden = (await hiddenResponse.json()) as ReadonlyArray<unknown>;
			expect(hidden).toHaveLength(0);

			const visibleResponse = await request(handler, {
				daemonToken,
				method: "POST",
				pathname: "/review/threads/list",
				body: {
					scope,
					filePath: null,
					includeResolved: false,
					includeStale: true,
					activeAnchors: [
						{
							anchorType: "line",
							filePath: "src/file.ts",
							lineSide: "new",
							lineNumber: 7,
							hunkHeader: null,
							lineContentHash: null,
						},
					],
				},
			});
			expect(visibleResponse.status).toBe(200);
			const visible = (await visibleResponse.json()) as ReadonlyArray<{
				readonly isStale: boolean;
			}>;
			expect(visible).toHaveLength(1);
			expect(visible[0]?.isStale).toBe(true);
		} finally {
			await dispose();
		}
	});
});
