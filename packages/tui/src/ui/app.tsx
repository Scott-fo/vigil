import { RGBA, type ScrollBoxRenderable } from "@opentui/core";
import { useKeyboard, useRenderer } from "@opentui/react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { splitDiffIntoHunkBlocks } from "#diff/hunks";
import {
	commitStagedChanges,
	isFileStaged,
	loadFilesWithDiffs,
	pullFromRemote,
	pushToRemote,
	toggleFileStage,
} from "#data/git";
import {
	cycleThemeName,
	resolveThemeBundle,
	type ResolvedTheme,
	type ThemeMode,
} from "#theme/theme";
import type { AppProps, FileEntry } from "#tui/types";

function getStatusColor(status: string, theme: ResolvedTheme) {
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

export function App(props: AppProps) {
	const renderer = useRenderer();
	const [themeName, setThemeName] = useState(props.initialThemeName);
	const [themeMode] = useState<ThemeMode>(props.initialThemeMode);
	const [files, setFiles] = useState<FileEntry[]>([]);
	const [selectedPath, setSelectedPath] = useState<string | null>(null);
	const [isCommitModalOpen, setIsCommitModalOpen] = useState(false);
	const [commitMessage, setCommitMessage] = useState("");
	const [commitError, setCommitError] = useState<string | null>(null);
	const [loading, setLoading] = useState(true);
	const [error, setError] = useState<string | null>(null);
	const isRefreshingRef = useRef(false);
	const diffScrollRef = useRef<ScrollBoxRenderable | null>(null);

	const themeBundle = useMemo(
		() => resolveThemeBundle(props.themeCatalog, themeName, themeMode),
		[props.themeCatalog, themeName, themeMode],
	);
	const theme = themeBundle.theme;
	const modalBackdropColor = RGBA.fromValues(
		theme.background.r,
		theme.background.g,
		theme.background.b,
		0.55,
	);

	const refreshFiles = useCallback(async (showLoading: boolean) => {
		if (isRefreshingRef.current) {
			return;
		}

		isRefreshingRef.current = true;
		if (showLoading) {
			setLoading(true);
		}
		try {
			const result = await loadFilesWithDiffs();

			setFiles(result.files);
			setError(result.error ?? null);

			setSelectedPath((current) => {
				if (result.files.length === 0) {
					return null;
				}
				if (current && result.files.some((file) => file.path === current)) {
					return current;
				}
				return result.files[0]?.path ?? null;
			});
		} finally {
			if (showLoading) {
				setLoading(false);
			}
			isRefreshingRef.current = false;
		}
	}, []);

	useEffect(() => {
		void refreshFiles(true);
	}, [refreshFiles]);

	useEffect(() => {
		const interval = setInterval(() => {
			void refreshFiles(false);
		}, 2000);

		return () => clearInterval(interval);
	}, [refreshFiles]);

	const selectedFile = useMemo(() => {
		if (files.length === 0) {
			return null;
		}
		if (selectedPath) {
			const match = files.find((file) => file.path === selectedPath);
			if (match) {
				return match;
			}
		}
		return files[0] ?? null;
	}, [files, selectedPath]);

	const selectedFileHunkDiffs = useMemo(() => {
		if (!selectedFile?.diff.trim()) {
			return [];
		}
		return splitDiffIntoHunkBlocks(selectedFile.diff);
	}, [selectedFile]);

	const selectedIndex = selectedFile
		? files.findIndex((file) => file.path === selectedFile.path)
		: -1;
	const stagedFileCount = useMemo(
		() => files.filter((file) => isFileStaged(file.status)).length,
		[files],
	);

	const submitCommit = useCallback(
		(rawMessage: string) => {
			const result = commitStagedChanges(rawMessage);
			if (!result.ok) {
				setCommitError(result.error ?? "Unable to create commit.");
				return;
			}

			setCommitMessage("");
			setCommitError(null);
			setIsCommitModalOpen(false);
			setError(null);
			void refreshFiles(false);
		},
		[refreshFiles],
	);

	useKeyboard((key) => {
		if (key.ctrl && key.name === "c") {
			renderer.destroy();
			return;
		}

		if (isCommitModalOpen) {
			if (key.name === "escape") {
				setIsCommitModalOpen(false);
				setCommitMessage("");
				setCommitError(null);
				return;
			}

			return;
		}

		if (
			key.name === "escape" ||
			key.name === "q"
		) {
			renderer.destroy();
			return;
		}

		if (!key.ctrl && !key.meta && key.name === "c") {
			if (stagedFileCount === 0) {
				return;
			}
			setCommitMessage("");
			setCommitError(null);
			setIsCommitModalOpen(true);
			setError(null);
			return;
		}

		if (!key.ctrl && !key.meta && key.name === "t") {
			setThemeName((current) =>
				cycleThemeName(props.themeCatalog, current, key.shift ? -1 : 1),
			);
			return;
		}

		if (!key.ctrl && !key.meta && key.name === "p") {
			const result = key.shift ? pushToRemote() : pullFromRemote();
			if (!result.ok) {
				setError(result.error ?? "Unable to sync with remote.");
				return;
			}

			setError(null);
			void refreshFiles(false);
			return;
		}

		if (key.ctrl && (key.name === "u" || key.name === "d")) {
			const step = Math.max(6, Math.floor(renderer.height * 0.45));
			diffScrollRef.current?.scrollBy({
				x: 0,
				y: key.name === "u" ? -step : step,
			});
			return;
		}

		if (files.length === 0 || selectedIndex === -1) {
			return;
		}

		if (
			!key.ctrl &&
			!key.meta &&
			(key.name === "space" || key.name === " ")
		) {
			const file = files[selectedIndex];
			if (!file) {
				return;
			}

			const result = toggleFileStage(file);
			if (!result.ok) {
				setError(result.error ?? "Unable to update staged state.");
				return;
			}

			setError(null);
			void refreshFiles(false);
			return;
		}

		if (key.name === "down" || key.name === "j") {
			const nextIndex = Math.min(selectedIndex + 1, files.length - 1);
			setSelectedPath(files[nextIndex]?.path ?? null);
			return;
		}

		if (key.name === "up" || key.name === "k") {
			const nextIndex = Math.max(selectedIndex - 1, 0);
			setSelectedPath(files[nextIndex]?.path ?? null);
		}
	});

	return (
		<box
			flexDirection="column"
			flexGrow={1}
			padding={1}
			backgroundColor={theme.background}
		>
			<box flexDirection="row" flexGrow={1}>
				<box
					width={44}
					border
					borderStyle="rounded"
					borderColor={theme.border}
					marginRight={1}
					flexDirection="column"
					backgroundColor={theme.backgroundPanel}
				>
					<box paddingX={1} marginBottom={1}>
						<text fg={theme.text}>
							<strong>Changed Files ({files.length})</strong>
						</text>
					</box>

					<scrollbox flexGrow={1}>
						{files.map((file) => {
							const selected = selectedFile?.path === file.path;
							const isStaged = isFileStaged(file.status);
							const rowBackground = selected
								? isStaged
									? theme.diffAddedLineNumberBg
									: theme.backgroundElement
								: isStaged
									? theme.diffAddedBg
									: "transparent";
							return (
								<box
									key={file.path}
									paddingX={1}
									backgroundColor={rowBackground}
									onMouseDown={(event) => {
										event.preventDefault();
										setSelectedPath(file.path);
									}}
								>
									<text>
										<span fg={getStatusColor(file.status, theme)}>
											{file.status}
										</span>{" "}
										<span
											fg={selected || isStaged ? theme.text : theme.textMuted}
										>
											{file.label}
										</span>
									</text>
								</box>
							);
						})}
					</scrollbox>
				</box>

				<box
					flexGrow={1}
					border
					borderStyle="rounded"
					borderColor={theme.border}
					flexDirection="column"
					backgroundColor={theme.backgroundPanel}
				>
					<box paddingX={1} marginBottom={1}>
						<text fg={theme.text}>
							<strong>
								{selectedFile ? selectedFile.label : "No file selected"}
							</strong>
						</text>
					</box>

					<box flexGrow={1} padding={1}>
						{loading ? (
							<text fg={theme.textMuted}>Loading git status...</text>
						) : error ? (
							<text fg={theme.error}>{error}</text>
						) : !selectedFile ? (
							<text fg={theme.textMuted}>No changed files found.</text>
						) : selectedFile.diff.trim() ? (
							<scrollbox
								ref={diffScrollRef}
								flexGrow={1}
								focused={!isCommitModalOpen}
								verticalScrollbarOptions={{
									trackOptions: {
										backgroundColor: theme.backgroundElement,
										foregroundColor: theme.borderActive,
									},
								}}
							>
								<box flexDirection="column">
									{selectedFileHunkDiffs.map((hunkDiff, hunkIndex) => (
										<box
											key={`${selectedFile.path}:${hunkIndex}`}
											flexDirection="column"
										>
											<diff
												diff={hunkDiff}
												filetype={selectedFile.filetype}
												syntaxStyle={themeBundle.syntaxStyle}
												view="split"
												showLineNumbers
												width="100%"
												wrapMode="word"
												fg={theme.text}
												addedBg={theme.diffAddedBg}
												removedBg={theme.diffRemovedBg}
												contextBg={theme.diffContextBg}
												addedSignColor={theme.diffHighlightAdded}
												removedSignColor={theme.diffHighlightRemoved}
												lineNumberFg={theme.diffLineNumber}
												lineNumberBg={theme.diffContextBg}
												addedLineNumberBg={theme.diffAddedLineNumberBg}
												removedLineNumberBg={theme.diffRemovedLineNumberBg}
											/>
											{hunkIndex < selectedFileHunkDiffs.length - 1 ? (
												<box
													height={1}
													backgroundColor={theme.background}
												/>
											) : null}
										</box>
									))}
								</box>
							</scrollbox>
						) : (
							<text fg={theme.textMuted}>
								{selectedFile.note ?? "No diff preview available."}
							</text>
						)}
					</box>
				</box>
			</box>

			{isCommitModalOpen ? (
				<box
					position="absolute"
					left={0}
					top={0}
					width="100%"
					height="100%"
					justifyContent="center"
					alignItems="center"
					backgroundColor={modalBackdropColor}
					zIndex={100}
				>
					<box
						width={72}
						border
						borderStyle="rounded"
						borderColor={theme.borderActive}
						backgroundColor={theme.backgroundPanel}
						padding={1}
						flexDirection="column"
					>
						<text fg={theme.text}>
							<strong>Commit Staged Changes</strong>
						</text>
						<box marginTop={1}>
							<input
								value={commitMessage}
								onChange={(value) => {
									setCommitMessage(value);
									if (commitError) {
										setCommitError(null);
									}
								}}
								onSubmit={(payload: unknown) => {
									if (typeof payload === "string") {
										submitCommit(payload);
										return;
									}
									submitCommit(commitMessage);
								}}
								placeholder="Enter commit message..."
								focused
								width="100%"
								backgroundColor={theme.backgroundElement}
								focusedBackgroundColor={theme.backgroundElement}
								textColor={theme.text}
								focusedTextColor={theme.text}
								placeholderColor={theme.textMuted}
							/>
						</box>
						<box marginTop={1}>
							{commitError ? (
								<text fg={theme.error}>{commitError}</text>
							) : (
								<text fg={theme.textMuted}>
									Enter commits. Esc closes without committing.
								</text>
							)}
						</box>
					</box>
				</box>
			) : null}
		</box>
	);
}
