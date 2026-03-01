import type { FileEntry } from "#tui/types";
import type { ResolvedTheme } from "#theme/theme";

export function getStatusColor(status: string, theme: ResolvedTheme) {
	if (status === "??" || status.includes("A")) {
		return theme.diffHighlightAdded;
	}
	if (status.includes("U") || status.includes("D")) {
		return theme.diffHighlightRemoved;
	}
	if (status.includes("R") || status.includes("C")) {
		return theme.accent;
	}
	if (status.includes("M")) {
		return theme.warning;
	}
	return theme.textMuted;
}

interface FileTreeNode {
	name: string;
	path: string;
	directories: Map<string, FileTreeNode>;
	files: Array<{ file: FileEntry; name: string }>;
}

export type SidebarItem =
	| {
			kind: "header";
			key: string;
			path: string;
			label: string;
			depth: number;
			collapsed: boolean;
	  }
	| {
			kind: "file";
			key: string;
			file: FileEntry;
			label: string;
			depth: number;
	  };

function createTreeNode(name: string, path: string): FileTreeNode {
	return {
		name,
		path,
		directories: new Map<string, FileTreeNode>(),
		files: [],
	};
}

function compareFileEntries(a: FileEntry, b: FileEntry): number {
	return a.path.localeCompare(b.path);
}

function displayNameFromPath(pathValue: string): string {
	const parts = pathValue.split("/").filter((part) => part.length > 0);
	return parts[parts.length - 1] ?? pathValue;
}

function getSidebarFileLabel(file: FileEntry, leafName: string): string {
	if (!file.label.includes(" -> ")) {
		return leafName;
	}

	const [fromRaw, toRaw] = file.label.split(" -> ");
	const fromName = displayNameFromPath(fromRaw ?? "");
	const toName = displayNameFromPath(toRaw ?? "");
	if (fromName && toName) {
		return `${fromName} -> ${toName}`;
	}
	return leafName;
}

export function buildSidebarItems(
	files: FileEntry[],
	collapsedDirectories: Set<string>,
): SidebarItem[] {
	const root = createTreeNode("", "");

	for (const file of files) {
		const parts = file.path.split("/").filter((part) => part.length > 0);
		const leafName = parts[parts.length - 1] ?? file.path;
		const sidebarLabel = getSidebarFileLabel(file, leafName);

		if (parts.length <= 1) {
			root.files.push({ file, name: sidebarLabel });
			continue;
		}

		let current = root;
		let currentPath = "";
		for (let index = 0; index < parts.length - 1; index += 1) {
			const name = parts[index] ?? "";
			currentPath = currentPath ? `${currentPath}/${name}` : name;

			let next = current.directories.get(name);
			if (!next) {
				next = createTreeNode(name, currentPath);
				current.directories.set(name, next);
			}
			current = next;
		}

		current.files.push({ file, name: sidebarLabel });
	}

	const items: SidebarItem[] = [];

	function compressDirectoryChain(start: FileTreeNode): {
		node: FileTreeNode;
		label: string;
	} {
		let node = start;
		const labelParts = [node.name];

		while (node.files.length === 0 && node.directories.size === 1) {
			const next = [...node.directories.values()][0];
			if (!next) {
				break;
			}

			node = next;
			labelParts.push(node.name);
		}

		return {
			node,
			label: labelParts.join("/"),
		};
	}

	function visit(node: FileTreeNode, depth: number) {
		const directories = [...node.directories.values()].sort((a, b) =>
			a.name.localeCompare(b.name),
		);

		for (const directory of directories) {
			const compact = compressDirectoryChain(directory);
			const isCollapsed = collapsedDirectories.has(compact.node.path);
			items.push({
				kind: "header",
				key: `h:${compact.node.path}`,
				path: compact.node.path,
				label: compact.label,
				depth,
				collapsed: isCollapsed,
			});
			if (!isCollapsed) {
				visit(compact.node, depth + 1);
			}
		}

		const nodeFiles = [...node.files].sort((a, b) =>
			compareFileEntries(a.file, b.file),
		);
		for (const fileEntry of nodeFiles) {
			items.push({
				kind: "file",
				key: `f:${fileEntry.file.path}`,
				file: fileEntry.file,
				label: fileEntry.name,
				depth,
			});
		}
	}

	visit(root, 0);
	return items;
}
