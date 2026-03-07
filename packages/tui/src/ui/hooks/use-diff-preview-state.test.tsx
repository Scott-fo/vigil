import { describe, expect, test } from "bun:test";
import { testRender } from "@opentui/react/test-utils";
import { Effect, Option } from "effect";
import { act, useEffect, useMemo, useState } from "react";
import type { FileDiffPreview } from "#data/git.ts";
import { FileEntry } from "#tui/types.ts";
import {
	buildDiffPrefetchPaths,
	type DiffPreviewLoaders,
	useDiffPreviewState,
} from "#ui/hooks/use-diff-preview-state.ts";
import type { ReviewMode } from "#ui/state.ts";

const reviewMode: ReviewMode = { _tag: "working-tree" };

function entry(path: string): FileEntry {
	return FileEntry.make({
		status: "M ",
		path,
		label: path,
	});
}

function preview(diff: string): FileDiffPreview {
	return {
		diff,
		note: Option.none(),
	};
}

function frameText(setup: Awaited<ReturnType<typeof testRender>>): string {
	return setup.captureCharFrame().replace(/\s+/g, " ").trim();
}

async function flushRender(setup: Awaited<ReturnType<typeof testRender>>) {
	await setup.renderOnce();
	await Promise.resolve();
	await setup.renderOnce();
}

function createLoaders() {
	let activeRefreshVersion = 0;

	const calls: Array<string> = [];

	const loaders: DiffPreviewLoaders = {
		loadWorkingTree: (file) => {
			const key = `${activeRefreshVersion}:${file.path}`;
			calls.push(key);
			return Effect.succeed(
				preview(`diff-${file.path}-v${activeRefreshVersion}`),
			);
		},
		loadBranchCompare: () => Effect.succeed(preview("unused")),
		loadCommitCompare: () => Effect.succeed(preview("unused")),
	};

	return {
		calls,
		loaders,
		setRefreshVersion: (version: number) => {
			activeRefreshVersion = version;
		},
	};
}

interface ProbeControls {
	readonly selectFile: (path: string) => void;
	readonly setRefreshVersion: (version: number) => void;
}

function DiffPreviewProbe(props: {
	readonly files: ReadonlyArray<FileEntry>;
	readonly initialSelectedPath: string;
	readonly loaders: DiffPreviewLoaders;
	readonly onReady: (controls: ProbeControls) => void;
}) {
	const [selectedPath, setSelectedPath] = useState(props.initialSelectedPath);
	const [pendingSelectedPath, setPendingSelectedPath] = useState<string | null>(
		null,
	);
	const [refreshVersion, setRefreshVersion] = useState(0);

	useEffect(() => {
		props.onReady({
			selectFile: setPendingSelectedPath,
			setRefreshVersion,
		});
	}, [props.onReady]);

	useEffect(() => {
		if (!pendingSelectedPath) {
			return;
		}

		setSelectedPath(pendingSelectedPath);
		setPendingSelectedPath(null);
	}, [pendingSelectedPath]);

	const visibleFilePaths = useMemo(
		() => props.files.map((file) => file.path),
		[props.files],
	);
	const selectedFile = useMemo(
		() => props.files.find((file) => file.path === selectedPath) ?? null,
		[props.files, selectedPath],
	);
	const selectedVisibleIndex = visibleFilePaths.indexOf(selectedPath);

	const { selectedFileDiff, selectedFileDiffLoading } = useDiffPreviewState({
		files: props.files,
		visibleFilePaths,
		selectedFile,
		selectedVisibleIndex,
		reviewMode,
		externalRefreshVersion: refreshVersion,
		loaders: props.loaders,
	});

	return (
		<box>
			<text>{selectedPath}</text>
			<text>{selectedFileDiffLoading ? "loading" : "ready"}</text>
			<text>{selectedFileDiff || "empty"}</text>
		</box>
	);
}

describe("buildDiffPrefetchPaths", () => {
	test("returns the selected file plus three above and below", () => {
		expect(
			buildDiffPrefetchPaths(
				["a.ts", "b.ts", "c.ts", "d.ts", "e.ts", "f.ts", "g.ts"],
				3,
			),
		).toEqual(["d.ts", "c.ts", "e.ts", "b.ts", "f.ts", "a.ts", "g.ts"]);
	});
});

describe("useDiffPreviewState", () => {
	test("warms the selected file plus three files above and below", async () => {
		const files = ["a.ts", "b.ts", "c.ts", "d.ts", "e.ts", "f.ts", "g.ts"].map(
			entry,
		);
		const loaderControl = createLoaders();

		const setup = await testRender(
			<DiffPreviewProbe
				files={files}
				initialSelectedPath="d.ts"
				loaders={loaderControl.loaders}
				onReady={() => {}}
			/>,
			{ width: 80, height: 10 },
		);

		try {
			await act(async () => {
				await flushRender(setup);
			});

			expect(loaderControl.calls).toEqual([
				"0:d.ts",
				"0:c.ts",
				"0:e.ts",
				"0:b.ts",
				"0:f.ts",
				"0:a.ts",
				"0:g.ts",
			]);
			expect(frameText(setup)).toContain("d.ts ready diff-d.ts-v0");
		} finally {
			await act(async () => {
				setup.renderer.destroy();
			});
		}
	});

	test("keeps the cached diff visible while a refreshed preview is requested", async () => {
		const files = ["a.ts", "b.ts", "c.ts", "d.ts"].map(entry);
		const loaderControl = createLoaders();
		let controls: ProbeControls | null = null;

		const setup = await testRender(
			<DiffPreviewProbe
				files={files}
				initialSelectedPath="b.ts"
				loaders={loaderControl.loaders}
				onReady={(nextControls) => {
					controls = nextControls;
				}}
			/>,
			{ width: 80, height: 10 },
		);

		try {
			await act(async () => {
				await flushRender(setup);
			});

			expect(frameText(setup)).toContain("b.ts ready diff-b.ts-v0");
			expect(controls).not.toBeNull();

			await act(async () => {
				if (!controls) {
					throw new Error("Probe controls were not initialized.");
				}

				loaderControl.setRefreshVersion(1);
				controls.setRefreshVersion(1);
				await setup.renderOnce();
			});

			expect(frameText(setup)).toContain("b.ts ready diff-b.ts-v0");
			expect(loaderControl.calls).toContain("1:b.ts");

			await act(async () => {
				await flushRender(setup);
			});

			expect(frameText(setup)).toContain("b.ts ready diff-b.ts-v1");
		} finally {
			await act(async () => {
				setup.renderer.destroy();
			});
		}
	});
});
