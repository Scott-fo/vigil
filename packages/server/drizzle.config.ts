import { resolveReviewsDatabasePath } from "@vigil/config";
import { defineConfig } from "drizzle-kit";

export default defineConfig({
	out: "./drizzle",
	schema: "./src/db/schema.ts",
	dialect: "sqlite",
	dbCredentials: {
		url: resolveReviewsDatabasePath(),
	},
	verbose: true,
	strict: true,
});
