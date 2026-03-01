import { Option, pipe } from "effect";

export function splitDiffIntoHunkBlocks(diff: string): string[] {
	const normalized = diff.replace(/\r\n/g, "\n");
	const lines = normalized.split("\n");
	const firstHunkIndex = lines.findIndex((line) => line.startsWith("@@ "));

	if (firstHunkIndex === -1) {
		return [diff];
	}

	const header = lines.slice(0, firstHunkIndex);
	const hunks: string[][] = [];
	let currentHunk = Option.none<Array<string>>();

	for (let index = firstHunkIndex; index < lines.length; index += 1) {
		const line = pipe(
			Option.fromNullable(lines[index]),
			Option.getOrElse(() => ""),
		);

		if (line.startsWith("@@ ")) {
			pipe(
				currentHunk,
				Option.filter((hunk) => hunk.length > 0),
				Option.match({
					onNone: () => {},
					onSome: (hunk) => {
						hunks.push(hunk);
					},
				}),
			);
			currentHunk = Option.some([line]);
			continue;
		}

		pipe(
			currentHunk,
			Option.match({
				onNone: () => {},
				onSome: (hunk) => {
					hunk.push(line);
				},
			}),
		);
	}

	pipe(
		currentHunk,
		Option.filter((hunk) => hunk.length > 0),
		Option.match({
			onNone: () => {},
			onSome: (hunk) => {
				hunks.push(hunk);
			},
		}),
	);

	if (hunks.length === 0) {
		return [diff];
	}

	return hunks.map((hunk) => [...header, ...hunk, ""].join("\n"));
}
