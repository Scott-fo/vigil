/** biome-ignore-all lint/a11y/noStaticElementInteractions: <opentui> */
import { Effect, pipe } from "effect";
import { RGBA, type ScrollBoxRenderable } from "@opentui/core";
import { useRenderer } from "@opentui/react";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { splitDiffIntoHunkBlocks } from "#diff/hunks";
import {
	commitStagedChanges,
	type RepoActionError,
	isFileStaged,
	loadFilesWithDiffs,
	pullFromRemote,
	pushToRemote,
	toggleFileStage,
} from "#data/git";
import {
	cycleThemeName,
	resolveThemeBundle,
	type ThemeMode,
} from "#theme/theme";
import type { AppProps, FileEntry } from "#tui/types";
import { useAppKeyboardInput } from "#ui/inputs";
import {
	buildSidebarItems,
	getStatusColor,
	type SidebarItem,
} from "#ui/sidebar";

function quoteShellArg(value: string): string {
	return `'${value.replace(/'/g, `'\"'\"'`)}'`;
}

function formatRepoActionError(error: RepoActionError): string {
	return pipe(
		Effect.fail(error),
		Effect.catchTags({
			CommitMessageRequiredError: (typedError) =>
				Effect.succeed(typedError.message),
			GitCommandError: (typedError) =>
				Effect.succeed(
					typedError.stderr.trim() ||
						typedError.stdout.trim() ||
						typedError.fallbackMessage,
				),
		}),
		Effect.runSync,
	);
}

export function App(props: AppProps) {
	const renderer = useRenderer();
	const [themeName, setThemeName] = useState(props.initialThemeName);
	const [themeMode] = useState<ThemeMode>(props.initialThemeMode);
	const [files, setFiles] = useState<FileEntry[]>([]);
	const [collapsedDirectories, setCollapsedDirectories] = useState<Set<string>>(
		() => new Set(),
	);
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
			const result = await Effect.runPromise(
				pipe(
					loadFilesWithDiffs(),
					Effect.match({
						onFailure: (repoError) => ({
							ok: false as const,
							error: formatRepoActionError(repoError),
						}),
						onSuccess: (nextFiles) => ({
							ok: true as const,
							files: nextFiles,
						}),
					}),
				),
			);
			if (!result.ok) {
				setFiles([]);
				setError(result.error);
				setSelectedPath(null);
				return;
			}

			setFiles(result.files);
			setError(null);

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

	const sidebarItems = useMemo(
		() => buildSidebarItems(files, collapsedDirectories),
		[collapsedDirectories, files],
	);
	const visibleFilePaths = useMemo(
		() =>
			sidebarItems
				.filter(
					(item): item is Extract<SidebarItem, { kind: "file" }> =>
						item.kind === "file",
				)
				.map((item) => item.file.path),
		[sidebarItems],
	);
	const selectedVisibleIndex = useMemo(() => {
		if (!selectedFile) {
			return -1;
		}
		return visibleFilePaths.indexOf(selectedFile.path);
	}, [selectedFile, visibleFilePaths]);
	const stagedFileCount = useMemo(
		() => files.filter((file) => isFileStaged(file.status)).length,
		[files],
	);

	useEffect(() => {
		if (visibleFilePaths.length === 0) {
			setSelectedPath(null);
			return;
		}

		if (selectedPath && visibleFilePaths.includes(selectedPath)) {
			return;
		}

		setSelectedPath(visibleFilePaths[0] ?? null);
	}, [selectedPath, visibleFilePaths]);

	const submitCommit = useCallback(
		(rawMessage: string) => {
			const result = Effect.runSync(
				pipe(
					commitStagedChanges(rawMessage),
					Effect.match({
						onFailure: (repoError) => ({
							ok: false as const,
							error: formatRepoActionError(repoError),
						}),
						onSuccess: () => ({ ok: true as const }),
					}),
				),
			);
			if (!result.ok) {
				setCommitError(result.error);
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

	const openSelectedFile = useCallback(
		async (filePath: string) => {
			if (props.chooserFilePath) {
				try {
					fs.writeFileSync(props.chooserFilePath, `${filePath}\n`, "utf8");
					renderer.destroy();
				} catch {
					setError(`Unable to write chooser file: ${props.chooserFilePath}`);
				}
				return;
			}

			const editorCommand = process.env.VISUAL ?? process.env.EDITOR;
			if (!editorCommand || editorCommand.trim().length === 0) {
				setError("Set VISUAL or EDITOR to open files from reviewer.");
				return;
			}

			renderer.suspend();
			try {
				const result = spawnSync(
					"sh",
					["-lc", `${editorCommand} ${quoteShellArg(filePath)}`],
					{ stdio: "inherit" },
				);

				if (result.error) {
					setError(result.error.message || "Failed to launch editor.");
					return;
				}

				if (result.status !== 0) {
					setError(`Editor command exited with code ${result.status ?? 1}.`);
					return;
				}

				setError(null);
			} catch {
				setError("Failed to launch editor from VISUAL/EDITOR.");
			} finally {
				renderer.resume();
				void refreshFiles(false);
			}
		},
		[props.chooserFilePath, refreshFiles, renderer],
	);

	const closeCommitModal = useCallback(() => {
		setIsCommitModalOpen(false);
		setCommitMessage("");
		setCommitError(null);
	}, []);

	const openCommitModal = useCallback(() => {
		if (stagedFileCount === 0) {
			return;
		}
		setCommitMessage("");
		setCommitError(null);
		setIsCommitModalOpen(true);
		setError(null);
	}, [stagedFileCount]);

	const cycleTheme = useCallback(
		(direction: 1 | -1) => {
			setThemeName((current) =>
				cycleThemeName(props.themeCatalog, current, direction),
			);
		},
		[props.themeCatalog],
	);

	const syncRemote = useCallback(
		(direction: "pull" | "push") => {
			const result = Effect.runSync(
				pipe(
					direction === "push" ? pushToRemote() : pullFromRemote(),
					Effect.match({
						onFailure: (repoError) => ({
							ok: false as const,
							error: formatRepoActionError(repoError),
						}),
						onSuccess: () => ({ ok: true as const }),
					}),
				),
			);
			if (!result.ok) {
				setError(result.error);
				return;
			}

			setError(null);
			void refreshFiles(false);
		},
		[refreshFiles],
	);

	const toggleSelectedFileStage = useCallback(
		(file: FileEntry) => {
			const result = Effect.runSync(
				pipe(
					toggleFileStage(file),
					Effect.match({
						onFailure: (repoError) => ({
							ok: false as const,
							error: formatRepoActionError(repoError),
						}),
						onSuccess: () => ({ ok: true as const }),
					}),
				),
			);
			if (!result.ok) {
				setError(result.error);
				return;
			}

			setError(null);
			void refreshFiles(false);
		},
		[refreshFiles],
	);

	const openSelectedFileFromKeyboard = useCallback(
		(filePath: string) => {
			void openSelectedFile(filePath);
		},
		[openSelectedFile],
	);

	useAppKeyboardInput({
		renderer,
		isCommitModalOpen,
		stagedFileCount,
		visibleFilePaths,
		selectedVisibleIndex,
		selectedFile,
		diffScrollRef,
		closeCommitModal,
		openCommitModal,
		cycleTheme,
		syncRemote,
		setSelectedPath,
		openSelectedFile: openSelectedFileFromKeyboard,
		toggleSelectedFileStage,
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
						{sidebarItems.map((item) => {
							if (item.kind === "header") {
								const headerPrefix = "  ".repeat(item.depth);
								return (
									<box
										key={item.key}
										paddingX={1}
										onMouseDown={(event) => {
											event.preventDefault();
											setCollapsedDirectories((current) => {
												const next = new Set(current);
												if (next.has(item.path)) {
													next.delete(item.path);
												} else {
													next.add(item.path);
												}
												return next;
											});
										}}
									>
										<text fg={theme.textMuted}>
											<span fg={theme.borderActive}>
												{headerPrefix}
												{item.collapsed ? "▸ " : "▾ "}
											</span>
											{item.label}
										</text>
									</box>
								);
							}

							const { file } = item;
							const selected = selectedFile?.path === file.path;
							const isStaged = isFileStaged(file.status);
							const rowBackground = selected
								? isStaged
									? theme.diffAddedLineNumberBg
									: theme.backgroundElement
								: isStaged
									? theme.diffAddedBg
									: "transparent";
							const filePrefix = "  ".repeat(item.depth);

							return (
								<box
									key={item.key}
									paddingX={1}
									backgroundColor={rowBackground}
									onMouseDown={(event) => {
										event.preventDefault();
										setSelectedPath(file.path);
									}}
								>
									<text>
										<span fg={theme.borderSubtle}>{filePrefix}</span>
										<span fg={getStatusColor(file.status, theme)}>
											{file.status}
										</span>{" "}
										<span
											fg={selected || isStaged ? theme.text : theme.textMuted}
										>
											{item.label}
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
												{...(selectedFile.filetype
													? { filetype: selectedFile.filetype }
													: {})}
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
												<box height={1} backgroundColor={theme.background} />
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
