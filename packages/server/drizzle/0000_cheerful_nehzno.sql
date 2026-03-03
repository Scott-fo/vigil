CREATE TABLE `review_comments` (
	`id` text PRIMARY KEY NOT NULL,
	`thread_id` text NOT NULL,
	`author` text DEFAULT 'local' NOT NULL,
	`body` text NOT NULL,
	`created_at_ms` integer NOT NULL,
	`updated_at_ms` integer NOT NULL,
	FOREIGN KEY (`thread_id`) REFERENCES `review_threads`(`id`) ON UPDATE no action ON DELETE cascade
);
--> statement-breakpoint
CREATE INDEX `review_comments_thread_idx` ON `review_comments` (`thread_id`);--> statement-breakpoint
CREATE INDEX `review_comments_thread_created_idx` ON `review_comments` (`thread_id`,`created_at_ms`);--> statement-breakpoint
CREATE TABLE `review_threads` (
	`id` text PRIMARY KEY NOT NULL,
	`repo_root` text NOT NULL,
	`scope_type` text NOT NULL,
	`scope_key` text NOT NULL,
	`source_ref` text,
	`destination_ref` text,
	`file_path` text,
	`line_side` text,
	`line_number` integer,
	`hunk_header` text,
	`line_content_hash` text,
	`is_resolved` integer DEFAULT false NOT NULL,
	`created_at_ms` integer NOT NULL,
	`updated_at_ms` integer NOT NULL
);
--> statement-breakpoint
CREATE INDEX `review_threads_scope_idx` ON `review_threads` (`repo_root`,`scope_key`);--> statement-breakpoint
CREATE INDEX `review_threads_file_idx` ON `review_threads` (`repo_root`,`scope_key`,`file_path`);--> statement-breakpoint
CREATE INDEX `review_threads_unresolved_idx` ON `review_threads` (`repo_root`,`scope_key`,`is_resolved`);