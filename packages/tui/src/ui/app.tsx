/** biome-ignore-all lint/a11y/noStaticElementInteractions: <opentui> */

import { Atom, useAtom } from "@effect-atom/atom-react";
import { RGBA, type ScrollBoxRenderable } from "@opentui/core";
import { useRenderer } from "@opentui/react";
import { Effect, Match, Option, pipe } from "effect";
import {
	memo,
	type RefObject,
	useCallback,
	useEffect,
	useMemo,
	useRef,
	useState,
} from "react";
import {
	openFileInEditor,
	renderOpenFileError,
	writeChooserSelection,
} from "#data/editor";
import {
	commitStagedChanges,
	isFileStaged,
	loadFilesWithDiffs,
	pullFromRemote,
	pushToRemote,
	type RepoActionError,
	toggleFileStage,
} from "#data/git";
import { splitDiffIntoHunkBlocks } from "#diff/hunks";
import {
	cycleThemeName,
	type ResolvedTheme,
	resolveThemeBundle,
	type ThemeBundle,
	type ThemeMode,
} from "#theme/theme";
import type { AppProps, FileEntry } from "#tui/types";
import { type AppKeyboardIntent, useAppKeyboardInput } from "#ui/inputs";
import {
	buildSidebarItems,
	getStatusColor,
	type SidebarItem,
} from "#ui/sidebar";

function areFileEntriesEqual(a: FileEntry[], b: FileEntry[]): boolean {
	if (a.length !== b.length) {
		return false;
	}

	for (let index = 0; index < a.length; index += 1) {
		const left = a[index];
		const right = b[index];
		if (!left || !right) {
			return false;
		}

		if (
			left.status !== right.status ||
			left.path !== right.path ||
			left.label !== right.label ||
			left.diff !== right.diff ||
			left.filetype !== right.filetype ||
			left.note !== right.note
		) {
			return false;
		}
	}

	return true;
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

interface SplashProps {
	theme: ResolvedTheme;
}

const Splash = memo(function Splash(props: SplashProps) {
	return (
		<box flexGrow={1} justifyContent="center" alignItems="center">
			<box flexDirection="column" rowGap={1} alignItems="center">
				<ascii-font text="reviewer" font="block" color={props.theme.text} />
				<text fg={props.theme.textMuted}>
					Initialise git repo to use Reviewer
				</text>
			</box>
		</box>
	);
});

interface ReviewerProps {
	theme: ResolvedTheme;
	themeBundle: ThemeBundle;
	files: FileEntry[];
	sidebarItems: SidebarItem[];
	selectedFile: FileEntry | null;
	loading: boolean;
	error: Option.Option<string>;
	isCommitModalOpen: boolean;
	diffScrollRef: RefObject<ScrollBoxRenderable | null>;
	onToggleDirectory: (path: string) => void;
	onSelectFilePath: (path: string) => void;
}

const Reviewer = memo(function Reviewer(props: ReviewerProps) {
	const selectedFileHunkDiffs = useMemo(() => {
		if (!props.selectedFile?.diff.trim()) {
			return [];
		}
		return splitDiffIntoHunkBlocks(props.selectedFile.diff);
	}, [props.selectedFile]);

	return (
		<box flexDirection="row" flexGrow={1}>
			<box
				width={38}
				border
				borderStyle="rounded"
				borderColor={props.theme.border}
				marginRight={1}
				flexDirection="column"
				backgroundColor={props.theme.backgroundPanel}
			>
				<box paddingX={1} marginBottom={1}>
					<text fg={props.theme.text}>
						<strong>Changed Files ({props.files.length})</strong>
					</text>
				</box>

				<scrollbox flexGrow={1}>
					{props.sidebarItems.map((item) => {
						if (item.kind === "header") {
							const headerPrefix = "  ".repeat(item.depth);
							return (
								<box
									key={item.key}
									paddingX={1}
									onMouseDown={(event) => {
										event.preventDefault();
										props.onToggleDirectory(item.path);
									}}
								>
									<text fg={props.theme.textMuted}>
										<span fg={props.theme.borderActive}>
											{headerPrefix}
											{item.collapsed ? "▸ " : "▾ "}
										</span>
										{item.label}
									</text>
								</box>
							);
						}

						const { file } = item;
						const selected = props.selectedFile?.path === file.path;
						const isStaged = isFileStaged(file.status);
						const rowBackground = selected
							? isStaged
								? props.theme.diffAddedLineNumberBg
								: props.theme.backgroundElement
							: isStaged
								? props.theme.diffAddedBg
								: "transparent";
						const filePrefix = "  ".repeat(item.depth);

						return (
							<box
								key={item.key}
								paddingX={1}
								backgroundColor={rowBackground}
								onMouseDown={(event) => {
									event.preventDefault();
									props.onSelectFilePath(file.path);
								}}
							>
								<text>
									<span fg={props.theme.borderSubtle}>{filePrefix}</span>
									<span fg={getStatusColor(file.status, props.theme)}>
										{file.status}
									</span>{" "}
									<span
										fg={
											selected || isStaged
												? props.theme.text
												: props.theme.textMuted
										}
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
				borderColor={props.theme.border}
				flexDirection="column"
				backgroundColor={props.theme.backgroundPanel}
			>
				<box paddingX={1} marginBottom={1}>
					<text fg={props.theme.text}>
						<strong>
							{props.selectedFile
								? props.selectedFile.label
								: "No file selected"}
						</strong>
					</text>
				</box>

				<box flexGrow={1} padding={1}>
					{props.loading ? (
						<text fg={props.theme.textMuted}>Loading git status...</text>
					) : Option.isSome(props.error) ? (
						<text fg={props.theme.error}>{props.error.value}</text>
					) : !props.selectedFile ? (
						<text fg={props.theme.textMuted}>No changed files found.</text>
					) : props.selectedFile.diff.trim() ? (
						<scrollbox
							ref={props.diffScrollRef}
							flexGrow={1}
							focused={!props.isCommitModalOpen}
							verticalScrollbarOptions={{
								trackOptions: {
									backgroundColor: props.theme.backgroundElement,
									foregroundColor: props.theme.borderActive,
								},
							}}
						>
							<box flexDirection="column">
								{selectedFileHunkDiffs.map((hunkDiff, hunkIndex) => (
									<box
										key={`${props.selectedFile?.path}:${hunkIndex}`}
										flexDirection="column"
									>
										<diff
											diff={hunkDiff}
											{...(props.selectedFile?.filetype
												? { filetype: props.selectedFile.filetype }
												: {})}
											syntaxStyle={props.themeBundle.syntaxStyle}
											view="split"
											showLineNumbers
											width="100%"
											wrapMode="word"
											fg={props.theme.text}
											addedBg={props.theme.diffAddedBg}
											removedBg={props.theme.diffRemovedBg}
											contextBg={props.theme.diffContextBg}
											addedSignColor={props.theme.diffHighlightAdded}
											removedSignColor={props.theme.diffHighlightRemoved}
											lineNumberFg={props.theme.diffLineNumber}
											lineNumberBg={props.theme.diffContextBg}
											addedLineNumberBg={props.theme.diffAddedLineNumberBg}
											removedLineNumberBg={props.theme.diffRemovedLineNumberBg}
										/>
										{hunkIndex < selectedFileHunkDiffs.length - 1 ? (
											<box
												height={1}
												backgroundColor={props.theme.background}
											/>
										) : null}
									</box>
								))}
							</box>
						</scrollbox>
					) : (
						<text fg={props.theme.textMuted}>
							{props.selectedFile.note ?? "No diff preview available."}
						</text>
					)}
				</box>
			</box>
		</box>
	);
});

interface CommitModalProps {
	theme: ResolvedTheme;
	modalBackdropColor: RGBA;
	commitMessage: string;
	commitError: Option.Option<string>;
	onCommitMessageChange: (value: string) => void;
	onCommitSubmit: (payload: unknown) => void;
}

const CommitModal = memo(function CommitModal(props: CommitModalProps) {
	return (
		<box
			position="absolute"
			left={0}
			top={0}
			width="100%"
			height="100%"
			justifyContent="center"
			alignItems="center"
			backgroundColor={props.modalBackdropColor}
			zIndex={100}
		>
			<box
				width={72}
				border
				borderStyle="rounded"
				borderColor={props.theme.borderActive}
				backgroundColor={props.theme.backgroundPanel}
				padding={1}
				flexDirection="column"
			>
				<text fg={props.theme.text}>
					<strong>Commit Staged Changes</strong>
				</text>
				<box marginTop={1}>
					<input
						value={props.commitMessage}
						onChange={props.onCommitMessageChange}
						onSubmit={props.onCommitSubmit}
						placeholder="Enter commit message..."
						focused
						width="100%"
						backgroundColor={props.theme.backgroundElement}
						focusedBackgroundColor={props.theme.backgroundElement}
						textColor={props.theme.text}
						focusedTextColor={props.theme.text}
						placeholderColor={props.theme.textMuted}
					/>
				</box>
				<box marginTop={1}>
					{Option.isSome(props.commitError) ? (
						<text fg={props.theme.error}>{props.commitError.value}</text>
					) : (
						<text fg={props.theme.textMuted}>
							Enter commits. Esc closes without committing.
						</text>
					)}
				</box>
			</box>
		</box>
	);
});

type UiStatus = {
	showSplash: boolean;
	error: Option.Option<string>;
};

const uiStatusAtom = Atom.make<UiStatus>({
	showSplash: true,
	error: Option.none(),
});

type CommitModalState =
	| {
			isOpen: false;
	  }
	| {
			isOpen: true;
			message: string;
			error: Option.Option<string>;
	  };

const commitModalAtom = Atom.make<CommitModalState>({
	isOpen: false,
});

export function App(props: AppProps) {
	const renderer = useRenderer();
	const [themeName, setThemeName] = useState(props.initialThemeName);
	const [themeMode] = useState<ThemeMode>(props.initialThemeMode);
	const [files, setFiles] = useState<FileEntry[]>([]);
	const [collapsedDirectories, setCollapsedDirectories] = useState<Set<string>>(
		() => new Set(),
	);
	const [selectedPath, setSelectedPath] = useState<Option.Option<string>>(
		Option.none(),
	);
	const [loading, setLoading] = useState(true);
	const [uiStatus, setUiStatus] = useAtom(uiStatusAtom);
	const [commitModal, setCommitModal] = useAtom(commitModalAtom);
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
	const isCommitModalOpen = commitModal.isOpen;
	const commitMessage = commitModal.isOpen ? commitModal.message : "";
	const commitError = commitModal.isOpen ? commitModal.error : Option.none();

	const refreshFiles = useCallback(
		async (showLoading: boolean) => {
			if (isRefreshingRef.current) {
				return;
			}

			isRefreshingRef.current = true;
			if (showLoading) {
				setLoading(true);
			}
			const result = await Effect.runPromise(
				pipe(
					loadFilesWithDiffs(),
					Effect.match({
						onFailure: (repoError) => ({
							ok: false as const,
							error: formatRepoActionError(repoError),
						}),
						onSuccess: (files) => ({
							ok: true as const,
							files,
						}),
					}),
					Effect.ensuring(
						Effect.sync(() => {
							if (showLoading) {
								setLoading(false);
							}
							isRefreshingRef.current = false;
						}),
					),
				),
			);
			if (!result.ok) {
				setFiles((current) => (current.length === 0 ? current : []));
				setUiStatus((current) => {
					if (
						current.showSplash &&
						Option.isSome(current.error) &&
						current.error.value === result.error
					) {
						return current;
					}
					return {
						showSplash: true,
						error: Option.some(result.error),
					};
				});
				setSelectedPath(Option.none());
				return;
			}

			setFiles((current) =>
				areFileEntriesEqual(current, result.files) ? current : result.files,
			);
			setUiStatus((current) => {
				if (!current.showSplash && Option.isNone(current.error)) {
					return current;
				}
				return {
					showSplash: false,
					error: Option.none(),
				};
			});

			setSelectedPath((current) => {
				if (result.files.length === 0) {
					return Option.none();
				}
				if (
					Option.isSome(current) &&
					result.files.some((file) => file.path === current.value)
				) {
					return current;
				}
				return pipe(
					Option.fromNullable(result.files[0]),
					Option.map((file) => file.path),
				);
			});
		},
		[setUiStatus],
	);

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
		const selectedFileMatch = pipe(
			selectedPath,
			Option.flatMap((path) =>
				Option.fromNullable(files.find((file) => file.path === path)),
			),
		);
		return pipe(
			selectedFileMatch,
			Option.getOrElse(() => files[0] ?? null),
		);
	}, [files, selectedPath]);

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
			setSelectedPath(Option.none());
			return;
		}

		if (
			Option.isSome(selectedPath) &&
			visibleFilePaths.includes(selectedPath.value)
		) {
			return;
		}

		setSelectedPath(Option.fromNullable(visibleFilePaths[0]));
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
				setCommitModal((current) =>
					current.isOpen
						? { ...current, error: Option.some(result.error) }
						: current,
				);
				return;
			}

			setCommitModal((current) =>
				current.isOpen ? { isOpen: false } : current,
			);
			setUiStatus((current) =>
				Option.isNone(current.error)
					? current
					: { ...current, error: Option.none() },
			);
			void refreshFiles(false);
		},
		[refreshFiles, setCommitModal, setUiStatus],
	);

	const openSelectedFile = useCallback(
		(filePath: string) => {
			if (Option.isSome(props.chooserFilePath)) {
				const chooserWriteResult = Effect.runSync(
					pipe(
						writeChooserSelection(props.chooserFilePath.value, filePath),
						Effect.match({
							onFailure: (error) => ({
								ok: false as const,
								error: renderOpenFileError(error),
							}),
							onSuccess: () => ({ ok: true as const }),
						}),
					),
				);
				if (!chooserWriteResult.ok) {
					setUiStatus((current) =>
						Option.isSome(current.error) &&
						current.error.value === chooserWriteResult.error
							? current
							: { ...current, error: Option.some(chooserWriteResult.error) },
					);
					return;
				}
				renderer.destroy();
				return;
			}

			renderer.suspend();
			const openResult = Effect.runSync(
				pipe(
					openFileInEditor(filePath),
					Effect.match({
						onFailure: (error) => ({
							ok: false as const,
							error: renderOpenFileError(error),
						}),
						onSuccess: () => ({ ok: true as const }),
					}),
				),
			);
			renderer.resume();
			if (!openResult.ok) {
				setUiStatus((current) =>
					Option.isSome(current.error) &&
					current.error.value === openResult.error
						? current
						: { ...current, error: Option.some(openResult.error) },
				);
				void refreshFiles(false);
				return;
			}
			setUiStatus((current) =>
				Option.isNone(current.error)
					? current
					: { ...current, error: Option.none() },
			);
			void refreshFiles(false);
		},
		[props.chooserFilePath, refreshFiles, renderer, setUiStatus],
	);

	const toggleCollapsedDirectory = useCallback((path: string) => {
		setCollapsedDirectories((current) => {
			const next = new Set(current);
			if (next.has(path)) {
				next.delete(path);
			} else {
				next.add(path);
			}
			return next;
		});
	}, []);

	const selectFilePath = useCallback((path: string) => {
		setSelectedPath(Option.some(path));
	}, []);

	const onCommitMessageChange = useCallback(
		(value: string) => {
			setCommitModal((current) => {
				if (!current.isOpen) {
					return current;
				}
				return {
					...current,
					message: value,
					error: Option.none(),
				};
			});
		},
		[setCommitModal],
	);

	const onCommitSubmit = useCallback(
		(payload: unknown) => {
			if (typeof payload === "string") {
				submitCommit(payload);
				return;
			}
			if (!commitModal.isOpen) {
				return;
			}
			submitCommit(commitModal.message);
		},
		[commitModal, submitCommit],
	);

	const closeCommitModal = useCallback(() => {
		setCommitModal((current) => (current.isOpen ? { isOpen: false } : current));
	}, [setCommitModal]);

	const openCommitModal = useCallback(() => {
		if (stagedFileCount === 0) {
			return;
		}
		setCommitModal({
			isOpen: true,
			message: "",
			error: Option.none(),
		});
		setUiStatus((current) =>
			Option.isNone(current.error)
				? current
				: { ...current, error: Option.none() },
		);
	}, [setCommitModal, stagedFileCount, setUiStatus]);

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
				setUiStatus((current) =>
					Option.isSome(current.error) && current.error.value === result.error
						? current
						: { ...current, error: Option.some(result.error) },
				);
				return;
			}

			setUiStatus((current) =>
				Option.isNone(current.error)
					? current
					: { ...current, error: Option.none() },
			);
			void refreshFiles(false);
		},
		[refreshFiles, setUiStatus],
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
				setUiStatus((current) =>
					Option.isSome(current.error) && current.error.value === result.error
						? current
						: { ...current, error: Option.some(result.error) },
				);
				return;
			}

			setUiStatus((current) =>
				Option.isNone(current.error)
					? current
					: { ...current, error: Option.none() },
			);
			void refreshFiles(false);
		},
		[refreshFiles, setUiStatus],
	);

	const onKeyboardIntent = useCallback(
		(intent: AppKeyboardIntent) => {
			return Match.value(intent).pipe(
				Match.tag("DestroyRenderer", () => {
					renderer.destroy();
					return;
				}),
				Match.tag("CloseCommitModal", () => {
					closeCommitModal();
					return;
				}),
				Match.tag("OpenCommitModal", () => {
					openCommitModal();
					return;
				}),
				Match.tag("CycleTheme", (typedIntent) => {
					cycleTheme(typedIntent.direction);
					return;
				}),
				Match.tag("SyncRemote", (typedIntent) => {
					syncRemote(typedIntent.direction);
					return;
				}),
				Match.tag("ScrollDiffHalfPage", (typedIntent) => {
					const step = Math.max(6, Math.floor(renderer.height * 0.45));
					diffScrollRef.current?.scrollBy({
						x: 0,
						y: typedIntent.direction === "up" ? -step : step,
					});
					return;
				}),
				Match.tag("OpenSelectedFile", (typedIntent) => {
					openSelectedFile(typedIntent.filePath);
					return;
				}),
				Match.tag("ToggleSelectedFileStage", (typedIntent) => {
					toggleSelectedFileStage(typedIntent.file);
					return;
				}),
				Match.tag("SelectVisiblePath", (typedIntent) => {
					setSelectedPath(Option.some(typedIntent.path));
					return;
				}),
				Match.exhaustive,
			);
		},
		[
			closeCommitModal,
			cycleTheme,
			openCommitModal,
			openSelectedFile,
			renderer,
			syncRemote,
			toggleSelectedFileStage,
		],
	);

	useAppKeyboardInput({
		isCommitModalOpen,
		stagedFileCount,
		visibleFilePaths,
		selectedVisibleIndex,
		selectedFile,
		onIntent: onKeyboardIntent,
	});

	return (
		<box
			flexDirection="column"
			flexGrow={1}
			padding={1}
			backgroundColor={theme.background}
		>
			{uiStatus.showSplash ? (
				<Splash theme={theme} />
			) : (
				<>
					<Reviewer
						theme={theme}
						themeBundle={themeBundle}
						files={files}
						sidebarItems={sidebarItems}
						selectedFile={selectedFile}
						loading={loading}
						error={uiStatus.error}
						isCommitModalOpen={isCommitModalOpen}
						diffScrollRef={diffScrollRef}
						onToggleDirectory={toggleCollapsedDirectory}
						onSelectFilePath={selectFilePath}
					/>
					{isCommitModalOpen && (
						<CommitModal
							theme={theme}
							modalBackdropColor={modalBackdropColor}
							commitMessage={commitMessage}
							commitError={commitError}
							onCommitMessageChange={onCommitMessageChange}
							onCommitSubmit={onCommitSubmit}
						/>
					)}
				</>
			)}
		</box>
	);
}
