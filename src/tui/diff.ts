export function splitDiffIntoHunkBlocks(diff: string): string[] {
	const normalized = diff.replace(/\r\n/g, "\n");
	const lines = normalized.split("\n");
	const firstHunkIndex = lines.findIndex((line) => line.startsWith("@@ "));

	if (firstHunkIndex === -1) {
		return [diff];
	}

	const header = lines.slice(0, firstHunkIndex);
	const hunks: string[][] = [];
	let currentHunk: string[] | null = null;

	for (let index = firstHunkIndex; index < lines.length; index += 1) {
		const line = lines[index] ?? "";

		if (line.startsWith("@@ ")) {
			if (currentHunk && currentHunk.length > 0) {
				hunks.push(currentHunk);
			}
			currentHunk = [line];
			continue;
		}

		if (currentHunk) {
			currentHunk.push(line);
		}
	}

	if (currentHunk && currentHunk.length > 0) {
		hunks.push(currentHunk);
	}

	if (hunks.length === 0) {
		return [diff];
	}

	return hunks.map((hunk) => [...header, ...hunk, ""].join("\n"));
}
