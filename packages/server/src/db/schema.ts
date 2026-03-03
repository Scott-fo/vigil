import { index, integer, sqliteTable, text } from "drizzle-orm/sqlite-core";

export const reviewThreadsTable = sqliteTable(
	"review_threads",
	{
		id: text("id").primaryKey(),
		repoRoot: text("repo_root").notNull(),
		scopeType: text("scope_type").notNull(),
		scopeKey: text("scope_key").notNull(),
		sourceRef: text("source_ref"),
		destinationRef: text("destination_ref"),
		filePath: text("file_path"),
		lineSide: text("line_side"),
		lineNumber: integer("line_number"),
		hunkHeader: text("hunk_header"),
		lineContentHash: text("line_content_hash"),
		isResolved: integer("is_resolved", { mode: "boolean" })
			.notNull()
			.default(false),
		createdAtMs: integer("created_at_ms").notNull(),
		updatedAtMs: integer("updated_at_ms").notNull(),
	},
	(table) => ({
		scopeIndex: index("review_threads_scope_idx").on(
			table.repoRoot,
			table.scopeKey,
		),
		fileIndex: index("review_threads_file_idx").on(
			table.repoRoot,
			table.scopeKey,
			table.filePath,
		),
		unresolvedIndex: index("review_threads_unresolved_idx").on(
			table.repoRoot,
			table.scopeKey,
			table.isResolved,
		),
	}),
);

export const reviewCommentsTable = sqliteTable(
	"review_comments",
	{
		id: text("id").primaryKey(),
		threadId: text("thread_id")
			.notNull()
			.references(() => reviewThreadsTable.id, {
				onDelete: "cascade",
			}),
		author: text("author").notNull().default("local"),
		body: text("body").notNull(),
		createdAtMs: integer("created_at_ms").notNull(),
		updatedAtMs: integer("updated_at_ms").notNull(),
	},
	(table) => ({
		threadIndex: index("review_comments_thread_idx").on(table.threadId),
		threadCreatedIndex: index("review_comments_thread_created_idx").on(
			table.threadId,
			table.createdAtMs,
		),
	}),
);
