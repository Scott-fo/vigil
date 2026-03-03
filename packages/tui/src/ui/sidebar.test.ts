import { describe, expect, test } from "bun:test";
import { buildSidebarItems } from "#ui/sidebar.ts";
import { FileEntry } from "#tui/types.ts";

function file(path: string, label = path): FileEntry {
	return FileEntry.make({
		status: "M ",
		path,
		label,
	});
}

describe("buildSidebarItems", () => {
	test("compresses directory chains and uses leaf file names", () => {
		const items = buildSidebarItems(
			[
				file("src/packages/app.tsx"),
				file("src/packages/utils.ts"),
				file("README.md"),
			],
			new Set(),
		);

		const header = items.find(
			(item) => item.kind === "header" && item.path === "src/packages",
		);
		expect(header).toBeDefined();
		if (!header || header.kind !== "header") {
			return;
		}

		expect(header.label).toBe("src/packages");
		expect(header.depth).toBe(0);

		const fileLabels = items
			.filter(
				(item): item is Extract<(typeof items)[number], { kind: "file" }> =>
					item.kind === "file",
			)
			.map((item) => item.label);

		expect(fileLabels).toContain("app.tsx");
		expect(fileLabels).toContain("utils.ts");
		expect(fileLabels).toContain("README.md");
	});

	test("omits children for collapsed directories", () => {
		const items = buildSidebarItems(
			[file("src/packages/app.tsx"), file("src/packages/utils.ts")],
			new Set(["src/packages"]),
		);

		const header = items.find(
			(item) => item.kind === "header" && item.path === "src/packages",
		);
		expect(header).toBeDefined();
		if (!header || header.kind !== "header") {
			return;
		}
		expect(header.collapsed).toBe(true);

		const nestedFiles = items.filter(
			(item) =>
				item.kind === "file" &&
				(item.file.path === "src/packages/app.tsx" ||
					item.file.path === "src/packages/utils.ts"),
		);
		expect(nestedFiles).toHaveLength(0);
	});

	test("uses renamed leaf label for rename entries", () => {
		const items = buildSidebarItems(
			[file("src/new-name.ts", "src/old-name.ts -> src/new-name.ts")],
			new Set(),
		);

		const renamed = items.find(
			(item) => item.kind === "file" && item.file.path === "src/new-name.ts",
		);
		expect(renamed).toBeDefined();
		if (!renamed || renamed.kind !== "file") {
			return;
		}

		expect(renamed.label).toBe("old-name.ts -> new-name.ts");
	});
});
