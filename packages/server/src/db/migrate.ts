import { resolveReviewsDatabasePath } from "@vigil/config";
import { Database } from "bun:sqlite";
import { mkdirSync } from "node:fs";
import path from "node:path";
import { drizzle } from "drizzle-orm/bun-sqlite";
import { migrate } from "drizzle-orm/bun-sqlite/migrator";
import { Data, Effect } from "effect";
import * as schema from "./schema.ts";

export class ReviewDatabaseDirectoryError extends Data.TaggedError(
	"ReviewDatabaseDirectoryError",
)<{
	readonly directoryPath: string;
	readonly message: string;
	readonly cause?: unknown;
}> {}

export class ReviewDatabaseOpenError extends Data.TaggedError(
	"ReviewDatabaseOpenError",
)<{
	readonly filePath: string;
	readonly message: string;
	readonly cause?: unknown;
}> {}

export class ReviewDatabaseMigrationError extends Data.TaggedError(
	"ReviewDatabaseMigrationError",
)<{
	readonly filePath: string;
	readonly migrationsFolder: string;
	readonly message: string;
	readonly cause?: unknown;
}> {}

export type ReviewDatabaseSetupError =
	| ReviewDatabaseDirectoryError
	| ReviewDatabaseOpenError
	| ReviewDatabaseMigrationError;

function toMessage(cause: unknown): string {
	return cause instanceof Error ? cause.message : String(cause);
}

function resolveMigrationsFolder(): string {
	return path.resolve(import.meta.dir, "..", "..", "drizzle");
}

export const migrateReviewDatabase = Effect.fn("ReviewDatabase.migrate")(
	function* () {
		const databaseFilePath = resolveReviewsDatabasePath();
		const databaseDirectoryPath = path.dirname(databaseFilePath);
		const migrationsFolder = resolveMigrationsFolder();

		yield* Effect.try({
			try: () => {
				mkdirSync(databaseDirectoryPath, { recursive: true });
			},
			catch: (cause) =>
				new ReviewDatabaseDirectoryError({
					directoryPath: databaseDirectoryPath,
					message: `Unable to create review database directory: ${toMessage(cause)}`,
					cause,
				}),
		});

		const sqlite = yield* Effect.acquireRelease(
			Effect.try({
				try: () =>
					new Database(databaseFilePath, {
						create: true,
					}),
				catch: (cause) =>
					new ReviewDatabaseOpenError({
						filePath: databaseFilePath,
						message: `Unable to open review database: ${toMessage(cause)}`,
						cause,
					}),
			}),
			(connection) =>
				Effect.sync(() => {
					connection.close();
				}),
		);

		yield* Effect.try({
			try: () => {
				const db = drizzle({
					client: sqlite,
					schema,
				});
				migrate(db, { migrationsFolder });
			},
			catch: (cause) =>
				new ReviewDatabaseMigrationError({
					filePath: databaseFilePath,
					migrationsFolder,
					message: `Unable to migrate review database: ${toMessage(cause)}`,
					cause,
				}),
		});

		yield* Effect.logInfo(`[review-db] migrated database=${databaseFilePath}`);
	},
);
