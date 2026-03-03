import { randomBytes } from "node:crypto";
import * as FileSystem from "@effect/platform/FileSystem";
import os from "node:os";
import path from "node:path";
import { Data, Effect, Schema, pipe } from "effect";

const TuiConfigObjectFromStringSchema = Schema.parseJson(
	Schema.Record({
		key: Schema.String,
		value: Schema.Unknown,
	}),
	{ space: 2 },
);

const decodeTuiConfigObject = Schema.decodeUnknown(TuiConfigObjectFromStringSchema);
const encodeTuiConfigObject = Schema.encode(TuiConfigObjectFromStringSchema);

export const TUI_CONFIG_FILE = "tui.json";
export const DAEMON_TOKEN_CONFIG_KEY = "daemon_token";
export const REVIEWS_DB_FILE = "reviews.sqlite";

export class TuiConfigReadError extends Data.TaggedError("TuiConfigReadError")<{
	readonly filePath: string;
	readonly message: string;
	readonly cause: unknown;
}> {}

export class TuiConfigParseError extends Data.TaggedError("TuiConfigParseError")<{
	readonly filePath: string;
	readonly message: string;
	readonly cause: unknown;
}> {}

export class TuiConfigWriteError extends Data.TaggedError("TuiConfigWriteError")<{
	readonly filePath: string;
	readonly message: string;
	readonly cause: unknown;
}> {}

export type ReadTuiConfigError = TuiConfigReadError | TuiConfigParseError;

export type WriteTuiConfigError = TuiConfigWriteError;

export type DaemonTokenConfigError =
	| TuiConfigReadError
	| TuiConfigParseError
	| TuiConfigWriteError;

export function resolveVigilDataDirectory(): string {
	return process.env.XDG_DATA_HOME
		? path.join(process.env.XDG_DATA_HOME, "vigil")
		: path.join(os.homedir(), ".local", "share", "vigil");
}

export function resolveTuiConfigPath(): string {
	return path.join(resolveVigilDataDirectory(), TUI_CONFIG_FILE);
}

export function resolveReviewsDatabasePath(): string {
	return path.join(resolveVigilDataDirectory(), REVIEWS_DB_FILE);
}

export function readTuiConfigObject(
	filePath: string = resolveTuiConfigPath(),
): Effect.Effect<
	Record<string, unknown>,
	ReadTuiConfigError,
	FileSystem.FileSystem
> {
	return Effect.gen(function* () {
		const fs = yield* FileSystem.FileSystem;
		const raw = yield* pipe(
			fs.readFileString(filePath),
			Effect.catchTag("SystemError", (cause) =>
				cause.reason === "NotFound"
					? Effect.succeed("")
					: Effect.fail(
							new TuiConfigReadError({
								filePath,
								message: cause.message,
								cause,
							}),
						),
			),
			Effect.catchTag("BadArgument", (cause) =>
				Effect.fail(
					new TuiConfigReadError({
						filePath,
						message: cause.message,
						cause,
					}),
				),
			),
		);

		if (raw.trim().length === 0) {
			return {} as Record<string, unknown>;
		}

		return yield* pipe(
			decodeTuiConfigObject(raw),
			Effect.mapError(
				(cause) =>
					new TuiConfigParseError({
						filePath,
						message: `Invalid ${TUI_CONFIG_FILE}.`,
						cause,
					}),
			),
		);
	});
}

export function writeTuiConfigObject(
	config: Record<string, unknown>,
	filePath: string = resolveTuiConfigPath(),
): Effect.Effect<void, WriteTuiConfigError, FileSystem.FileSystem> {
	return Effect.gen(function* () {
		const fs = yield* FileSystem.FileSystem;

		yield* pipe(
			fs.makeDirectory(path.dirname(filePath), { recursive: true }),
			Effect.catchTag("SystemError", (cause) =>
				Effect.fail(
					new TuiConfigWriteError({
						filePath,
						message: cause.message,
						cause,
					}),
				),
			),
			Effect.catchTag("BadArgument", (cause) =>
				Effect.fail(
					new TuiConfigWriteError({
						filePath,
						message: cause.message,
						cause,
					}),
				),
			),
		);

		const encoded = yield* pipe(
			encodeTuiConfigObject(config),
			Effect.mapError(
				(cause) =>
					new TuiConfigWriteError({
						filePath,
						message: `Unable to encode ${TUI_CONFIG_FILE}.`,
						cause,
					}),
			),
		);

		yield* pipe(
			fs.writeFileString(filePath, `${encoded}\n`),
			Effect.catchTag("SystemError", (cause) =>
				Effect.fail(
					new TuiConfigWriteError({
						filePath,
						message: cause.message,
						cause,
					}),
				),
			),
			Effect.catchTag("BadArgument", (cause) =>
				Effect.fail(
					new TuiConfigWriteError({
						filePath,
						message: cause.message,
						cause,
					}),
				),
			),
		);
	});
}

function generateDaemonToken(): string {
	return randomBytes(32).toString("hex");
}

export function loadOrCreateDaemonTokenFromTuiConfig(
	filePath: string = resolveTuiConfigPath(),
): Effect.Effect<string, DaemonTokenConfigError, FileSystem.FileSystem> {
	return Effect.gen(function* () {
		const config = yield* readTuiConfigObject(filePath);
		const existing = config[DAEMON_TOKEN_CONFIG_KEY];

		if (typeof existing === "string" && existing.trim().length > 0) {
			return existing.trim();
		}

		const daemonToken = generateDaemonToken();
		yield* writeTuiConfigObject(
			{
				...config,
				[DAEMON_TOKEN_CONFIG_KEY]: daemonToken,
			},
			filePath,
		);

		return daemonToken;
	});
}
