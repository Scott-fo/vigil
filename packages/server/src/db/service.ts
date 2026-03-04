import { resolveReviewsDatabasePath } from "@vigil/config";
import { Database } from "bun:sqlite";
import { existsSync, mkdirSync, readdirSync } from "node:fs";
import path from "node:path";
import { drizzle } from "drizzle-orm/bun-sqlite";
import { migrate } from "drizzle-orm/bun-sqlite/migrator";
import { Context, Effect, Layer, Schema } from "effect";
import * as schema from "./schema.ts";

export class DbError extends Schema.TaggedError<DbError>()("DbError", {
	message: Schema.String,
}) {}

export type Db = ReturnType<typeof drizzle<typeof schema>>;

export interface DbServiceShape {
	readonly client: Db;
	readonly use: <A>(
		fn: (db: Db) => A | Promise<A>,
		spanName?: string,
	) => Effect.Effect<A, DbError, never>;
}

function toMessage(cause: unknown): string {
	return cause instanceof Error ? cause.message : String(cause);
}

function resolveMigrationsFolder(): string {
	return path.resolve(import.meta.dir, "..", "..", "drizzle");
}

function hasSqlMigrations(migrationsFolder: string): boolean {
	if (!existsSync(migrationsFolder)) {
		return false;
	}

	return readdirSync(migrationsFolder).some((entry) => entry.endsWith(".sql"));
}

function makeDbService(db: Db): DbServiceShape {
	const use: DbServiceShape["use"] = (fn, spanName) =>
		Effect.tryPromise({
			try: () => Promise.resolve().then(() => fn(db)),
			catch: (cause) =>
				DbError.make({
					message: toMessage(cause),
				}),
		}).pipe(Effect.withSpan(`db.${(spanName ?? fn.name) || "use"}`));

	return {
		client: db,
		use,
	};
}

export class DbService extends Context.Tag("@vigil/server/DbService")<
	DbService,
	DbServiceShape
>() {
	static readonly layer = Layer.scoped(
		DbService,
		Effect.gen(function* () {
			const databaseFilePath = resolveReviewsDatabasePath();
			const databaseDirectoryPath = path.dirname(databaseFilePath);
			const migrationsFolder = resolveMigrationsFolder();

			yield* Effect.try({
				try: () => {
					mkdirSync(databaseDirectoryPath, { recursive: true });
					mkdirSync(migrationsFolder, { recursive: true });
				},
				catch: (cause) =>
					DbError.make({
						message: `Unable to create review database directory: ${toMessage(cause)}`,
					}),
			});

			const sqlite = yield* Effect.acquireRelease(
				Effect.try({
					try: () =>
						new Database(databaseFilePath, {
							create: true,
						}),
					catch: (cause) =>
						DbError.make({
							message: `Unable to open review database: ${toMessage(cause)}`,
						}),
				}),
				(connection) =>
					Effect.sync(() => {
						connection.close();
					}),
			);

			const client = drizzle({
				client: sqlite,
				schema,
			});

			if (hasSqlMigrations(migrationsFolder)) {
				yield* Effect.try({
					try: () => {
						migrate(client, { migrationsFolder });
					},
					catch: (cause) =>
						DbError.make({
							message: `Unable to migrate review database: ${toMessage(cause)}`,
						}),
				});

				yield* Effect.logInfo(`[review-db] migrated database=${databaseFilePath}`);
			} else {
				yield* Effect.logInfo(
					`[review-db] no migrations found database=${databaseFilePath}`,
				);
			}

			return DbService.of(makeDbService(client));
		}),
	);
}
