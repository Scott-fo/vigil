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
	const dataHome = await mkdtemp(path.join(tmpdir(), "vigil-support-api-data-"));
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

describe("support api", () => {
	const daemonToken = "support-api-test-token";
	const cleanupPaths: Array<string> = [];

	afterEach(async () => {
		await Promise.all(
			cleanupPaths.splice(0).map((directoryPath) =>
				rm(directoryPath, { recursive: true, force: true }),
			),
		);
	});

	test("review-diff rejects invalid payload", async () => {
		const { handler, dispose, dataHome } = await makeTestHandler(daemonToken);
		cleanupPaths.push(dataHome);

		try {
			const response = await handler(
				new Request("http://localhost/support/review-diff", {
					method: "POST",
					headers: {
						[VIGIL_DAEMON_TOKEN_HEADER]: daemonToken,
						"content-type": "application/json",
					},
					body: JSON.stringify({
						repoRoot: "",
						mode: "working-tree",
						sourceRef: null,
						destinationRef: null,
					}),
				}),
			);

			expect(response.status).toBe(400);
		} finally {
			await dispose();
		}
	});
});
