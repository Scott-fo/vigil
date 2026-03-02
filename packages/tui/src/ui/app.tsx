/** biome-ignore-all lint/a11y/noStaticElementInteractions: <opentui> */

import { useAtom } from "@effect-atom/atom-react";
import {
	RGBA,
	type ScrollBoxRenderable,
	type SyntaxStyle,
} from "@opentui/core";
import { useRenderer } from "@opentui/react";
import { Effect, Match, Option, pipe } from "effect";
import {
	type RefObject,
	useCallback,
	useEffect,
	useMemo,
	useRef,
	useState,
} from "react";
import { copyTextToClipboard } from "#data/clipboard";
import {
	isFileStaged,
	loadBranchFilePreview,
	loadFilePreview,
	type FileDiffPreview,
	type RepoActionError,
} from "#data/git";
import { type ResolvedTheme, resolveThemeBundle, type ThemeMode } from "#theme/theme";
import type { AppProps, FileEntry } from "#tui/types";
import { CommitModal } from "#ui/components/commit-modal";
import { DiscardModal } from "#ui/components/discard-modal";
import { HelpModal } from "#ui/components/help-modal";
import { RemoteSyncStatus } from "#ui/components/remote-sync-status";
import { Reviewer } from "#ui/components/reviewer";
import { Snackbar, type SnackbarNotice } from "#ui/components/snackbar";
import { Splash } from "#ui/components/splash";
import { ThemeModal } from "#ui/components/theme-modal";
import { useFileRefresh } from "#ui/hooks/use-file-refresh";
import { useRepoActions } from "#ui/hooks/use-repo-actions";
import { useAppKeyboardInput } from "#ui/inputs";
import { buildSidebarItems, type SidebarItem } from "#ui/sidebar";
import {
	commitModalAtom,
	discardModalAtom,
	fileViewStateAtom,
	helpModalAtom,
	remoteSyncAtom,
	reviewModeAtom,
	type RemoteSyncState,
	themeModalAtom,
	type UpdateCommitModal,
	type UpdateDiscardModal,
	type UpdateFileViewState,
	type UpdateHelpModal,
	type UpdateRemoteSyncState,
	type UpdateThemeModal,
	type UpdateUiStatus,
	uiStatusAtom,
} from "#ui/state";

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

interface AppContentProps {
	readonly theme: ResolvedTheme;
	readonly uiShowSplash: boolean;
	readonly uiError: Option.Option<string>;
	readonly isCommitModalOpen: boolean;
	readonly isDiscardModalOpen: boolean;
	readonly isHelpModalOpen: boolean;
	readonly isThemeModalOpen: boolean;
	readonly modalBackdropColor: RGBA;
	readonly commitMessage: string;
	readonly commitError: Option.Option<string>;
	readonly files: FileEntry[];
	readonly sidebarItems: SidebarItem[];
	readonly remoteSync: RemoteSyncState;
	readonly selectedFile: FileEntry | null;
	readonly selectedFileDiff: string;
	readonly selectedFileDiffNote: Option.Option<string>;
	readonly selectedFileDiffLoading: boolean;
	readonly loading: boolean;
	readonly diffViewMode: "split" | "unified";
	readonly syntaxStyle: SyntaxStyle;
	readonly diffScrollRef: RefObject<ScrollBoxRenderable | null>;
	readonly onToggleDirectory: (path: string) => void;
	readonly onSelectFilePath: (path: string) => void;
	readonly sidebarOpen: boolean;
	readonly onToggleSidebar: () => void;
	readonly onCommitMessageChange: (value: string) => void;
	readonly onCommitSubmit: (payload: unknown) => void;
	readonly snackbarNotice: Option.Option<SnackbarNotice>;
	readonly onCopySelection: () => void;
	readonly themeNames: ReadonlyArray<string>;
	readonly selectedThemeName: string;
	readonly themeSearchQuery: string;
	readonly onThemeSearchQueryChange: (value: string) => void;
	readonly onSelectThemeInModal: (themeName: string) => void;
	readonly discardModalFile: FileEntry | null;
	readonly onCancelDiscardModal: () => void;
	readonly onConfirmDiscardModal: () => void;
}

function AppContent(props: AppContentProps) {
	return (
		<box
			flexDirection="column"
			flexGrow={1}
			padding={1}
			backgroundColor={props.theme.background}
		>
			{props.uiShowSplash ? (
				<Splash theme={props.theme} error={props.uiError} />
			) : (
				<Reviewer
					theme={props.theme}
					syntaxStyle={props.syntaxStyle}
					files={props.files}
					sidebarItems={props.sidebarItems}
					selectedFile={props.selectedFile}
					selectedFileDiff={props.selectedFileDiff}
					selectedFileDiffNote={props.selectedFileDiffNote}
					selectedFileDiffLoading={props.selectedFileDiffLoading}
					loading={props.loading}
					diffViewMode={props.diffViewMode}
					error={props.uiError}
					isCommitModalOpen={
						props.isCommitModalOpen ||
						props.isDiscardModalOpen ||
						props.isHelpModalOpen ||
						props.isThemeModalOpen
					}
					diffScrollRef={props.diffScrollRef}
					onToggleDirectory={props.onToggleDirectory}
					onSelectFilePath={props.onSelectFilePath}
					sidebarOpen={props.sidebarOpen}
					onToggleSidebar={props.onToggleSidebar}
					onCopySelection={props.onCopySelection}
				/>
			)}
			{props.isCommitModalOpen && (
				<CommitModal
					theme={props.theme}
					modalBackdropColor={props.modalBackdropColor}
					commitMessage={props.commitMessage}
					commitError={props.commitError}
					onCommitMessageChange={props.onCommitMessageChange}
					onCommitSubmit={props.onCommitSubmit}
				/>
			)}
			{props.isDiscardModalOpen && props.discardModalFile && (
				<DiscardModal
					theme={props.theme}
					modalBackdropColor={props.modalBackdropColor}
					file={props.discardModalFile}
					onCancel={props.onCancelDiscardModal}
					onConfirm={props.onConfirmDiscardModal}
				/>
			)}
			{props.isHelpModalOpen && (
				<HelpModal
					theme={props.theme}
					modalBackdropColor={props.modalBackdropColor}
				/>
			)}
			{props.isThemeModalOpen && (
				<ThemeModal
					theme={props.theme}
					modalBackdropColor={props.modalBackdropColor}
					themes={props.themeNames}
					selectedThemeName={props.selectedThemeName}
					searchQuery={props.themeSearchQuery}
					onSearchQueryChange={props.onThemeSearchQueryChange}
					onSelectTheme={props.onSelectThemeInModal}
				/>
			)}
			<RemoteSyncStatus theme={props.theme} state={props.remoteSync} />
			<Snackbar
				theme={props.theme}
				notice={props.snackbarNotice}
				top={props.remoteSync._tag === "running" ? 4 : 1}
			/>
		</box>
	);
}

export function App(props: AppProps) {
	const renderer = useRenderer();
	const [themeName, setThemeName] = useState(props.initialThemeName);
	const [themeSearchQuery, setThemeSearchQuery] = useState("");
	const [themeMode] = useState<ThemeMode>(props.initialThemeMode);
	const [fileView, setFileView] = useAtom(fileViewStateAtom);
	const [uiStatus, setUiStatus] = useAtom(uiStatusAtom);
	const [commitModal, setCommitModal] = useAtom(commitModalAtom);
	const [discardModal, setDiscardModal] = useAtom(discardModalAtom);
	const [helpModal, setHelpModal] = useAtom(helpModalAtom);
	const [themeModal, setThemeModal] = useAtom(themeModalAtom);
	const [remoteSync, setRemoteSync] = useAtom(remoteSyncAtom);
	const [reviewMode] = useAtom(reviewModeAtom);
	const [snackbarNotice, setSnackbarNotice] = useState<
		Option.Option<SnackbarNotice>
	>(Option.none());
	const [selectedFilePreview, setSelectedFilePreview] = useState<{
		path: string;
		status: string;
		loading: boolean;
		preview: FileDiffPreview;
	} | null>(null);
	const diffScrollRef = useRef<ScrollBoxRenderable | null>(null);
	const snackbarTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
	const filePreviewCacheRef = useRef(
		new Map<
			string,
			{
				status: string;
				preview: FileDiffPreview;
			}
		>(),
	);

	const updateFileView = useCallback<UpdateFileViewState>(
		(update) => {
			setFileView(update);
		},
		[setFileView],
	);
	const updateUiStatus = useCallback<UpdateUiStatus>(
		(update) => {
			setUiStatus(update);
		},
		[setUiStatus],
	);
	const updateCommitModal = useCallback<UpdateCommitModal>(
		(update) => {
			setCommitModal(update);
		},
		[setCommitModal],
	);
	const updateHelpModal = useCallback<UpdateHelpModal>(
		(update) => {
			setHelpModal(update);
		},
		[setHelpModal],
	);
	const updateThemeModal = useCallback<UpdateThemeModal>(
		(update) => {
			setThemeModal(update);
		},
		[setThemeModal],
	);
	const updateRemoteSync = useCallback<UpdateRemoteSyncState>(
		(update) => {
			setRemoteSync(update);
		},
		[setRemoteSync],
	);
	const updateDiscardModal = useCallback<UpdateDiscardModal>(
		(update) => {
			setDiscardModal(update);
		},
		[setDiscardModal],
	);

	const {
		files,
		sidebarOpen,
		diffViewMode,
		collapsedDirectories,
		selectedPath,
		loading,
	} = fileView;
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
	const isDiscardModalOpen = discardModal.isOpen;
	const isHelpModalOpen = helpModal.isOpen;
	const isThemeModalOpen = themeModal.isOpen;
	const discardModalFile = discardModal.isOpen ? discardModal.file : null;
	const selectedThemeName = themeModal.isOpen
		? themeModal.selectedThemeName
		: themeName;
	const filteredThemeNames = useMemo(() => {
		const query = themeSearchQuery.trim().toLowerCase();
		if (query.length === 0) {
			return props.themeCatalog.order;
		}
		return props.themeCatalog.order.filter((themeCatalogName) =>
			themeCatalogName.toLowerCase().includes(query),
		);
	}, [props.themeCatalog.order, themeSearchQuery]);
	const commitMessage = commitModal.isOpen ? commitModal.message : "";
	const commitError = commitModal.isOpen ? commitModal.error : Option.none();
	const canInitializeGitRepo = pipe(
		uiStatus.error,
		Option.match({
			onNone: () => false,
			onSome: (error) =>
				uiStatus.showSplash && /not a git repository/i.test(error),
		}),
	);

	const { refreshFiles } = useFileRefresh({
		updateFileView,
		updateUiStatus,
		renderRepoActionError: formatRepoActionError,
		reviewMode,
		pollMs: 2000,
		pollingEnabled: remoteSync._tag !== "running",
	});

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
	const showSnackbar = useCallback((notice: SnackbarNotice) => {
		if (snackbarTimeoutRef.current) {
			clearTimeout(snackbarTimeoutRef.current);
		}
		setSnackbarNotice(Option.some(notice));
		const timeoutHandle = setTimeout(() => {
			setSnackbarNotice(Option.none());
		}, 2000);
		timeoutHandle.unref?.();
		snackbarTimeoutRef.current = timeoutHandle;
	}, []);

	const copyAndNotify = useCallback(
		(text: string) => {
			if (text.length === 0) {
				return;
			}

			void Effect.runPromise(
				pipe(
					copyTextToClipboard(renderer, text),
					Effect.match({
						onFailure: (error) => {
							showSnackbar({
								message: Match.value(error).pipe(
									Match.tag(
										"NativeClipboardCopyError",
										(typedError) => typedError.message,
									),
									Match.tag(
										"ClipboardUnavailableError",
										(typedError) => typedError.message,
									),
									Match.exhaustive,
								),
								variant: "error",
							});
						},
						onSuccess: () => {
							showSnackbar({
								message: "Text copied to clipboard",
								variant: "info",
							});
						},
					}),
				),
			);
		},
		[renderer, showSnackbar],
	);

	const onCopySelection = useCallback(() => {
		const text = renderer.getSelection()?.getSelectedText();
		if (!text) {
			return;
		}
		copyAndNotify(text);
		renderer.clearSelection();
	}, [copyAndNotify, renderer]);

	useEffect(() => {
		filePreviewCacheRef.current.clear();
		setSelectedFilePreview(null);
	}, [reviewMode]);

	useEffect(() => {
		if (!selectedFile) {
			setSelectedFilePreview(null);
			return;
		}

		const cachedPreview = filePreviewCacheRef.current.get(selectedFile.path);
		if (cachedPreview && cachedPreview.status === selectedFile.status) {
			setSelectedFilePreview({
				path: selectedFile.path,
				status: selectedFile.status,
				loading: false,
				preview: cachedPreview.preview,
			});
			return;
		}

		setSelectedFilePreview({
			path: selectedFile.path,
			status: selectedFile.status,
			loading: true,
			preview: { diff: "", note: Option.none() },
		});

		let cancelled = false;
		const previewEffect =
			reviewMode._tag === "working-tree"
				? loadFilePreview(selectedFile)
				: loadBranchFilePreview(selectedFile.path, reviewMode.selection);
		void Effect.runPromise(previewEffect).then((preview) => {
			if (cancelled) {
				return;
			}
			filePreviewCacheRef.current.set(selectedFile.path, {
				status: selectedFile.status,
				preview,
			});
			setSelectedFilePreview({
				path: selectedFile.path,
				status: selectedFile.status,
				loading: false,
				preview,
			});
		});

		return () => {
			cancelled = true;
		};
	}, [reviewMode, selectedFile]);

	useEffect(() => {
		const visiblePaths = new Set(files.map((file) => file.path));
		const cache = filePreviewCacheRef.current;
		for (const [path] of cache) {
			if (!visiblePaths.has(path)) {
				cache.delete(path);
			}
		}
	}, [files]);

	useEffect(() => {
		if (!isThemeModalOpen) {
			return;
		}
		setThemeSearchQuery("");
	}, [isThemeModalOpen]);

	useEffect(() => {
		updateFileView((current) => {
			if (visibleFilePaths.length === 0) {
				return Option.isNone(current.selectedPath)
					? current
					: { ...current, selectedPath: Option.none() };
			}
			if (
				Option.isSome(current.selectedPath) &&
				visibleFilePaths.includes(current.selectedPath.value)
			) {
				return current;
			}
			return {
				...current,
				selectedPath: Option.fromNullable(visibleFilePaths[0]),
			};
		});
	}, [updateFileView, visibleFilePaths]);

	useEffect(
		() => () => {
			if (snackbarTimeoutRef.current) {
				clearTimeout(snackbarTimeoutRef.current);
			}
		},
		[],
	);

	useEffect(() => {
		renderer.console.onCopySelection = (text: string) => {
			if (!text) {
				return;
			}
			copyAndNotify(text);
			renderer.clearSelection();
		};
		return () => {
			renderer.console.onCopySelection = undefined;
		};
	}, [copyAndNotify, renderer]);

	const {
		onCommitMessageChange,
		onCommitSubmit,
		onCancelDiscardModal,
		onConfirmDiscardModal,
		onKeyboardIntent,
		onToggleDirectory,
		onSelectFilePath,
		onSelectThemeInModal,
		onToggleSidebar,
	} = useRepoActions({
		chooserFilePath: props.chooserFilePath,
		renderer,
		diffScrollRef,
		themeName,
		themeMode,
		themeCatalog: props.themeCatalog,
		themeModalThemeNames: filteredThemeNames,
		setThemeName,
		stagedFileCount,
		commitModal,
		discardModal,
		helpModal,
		themeModal,
		canInitializeGitRepo,
		updateFileView,
		updateUiStatus,
		updateCommitModal,
		updateHelpModal,
		updateThemeModal,
		remoteSync,
		updateRemoteSync,
		updateDiscardModal,
		refreshFiles,
		renderRepoActionError: formatRepoActionError,
	});

	useEffect(() => {
		if (!isThemeModalOpen || filteredThemeNames.length === 0) {
			return;
		}
		if (filteredThemeNames.includes(selectedThemeName)) {
			return;
		}
		const firstFilteredThemeName = filteredThemeNames[0];
		if (firstFilteredThemeName) {
			onSelectThemeInModal(firstFilteredThemeName);
		}
	}, [
		filteredThemeNames,
		isThemeModalOpen,
		onSelectThemeInModal,
		selectedThemeName,
	]);

	useAppKeyboardInput({
		isCommitModalOpen,
		isDiscardModalOpen,
		isHelpModalOpen,
		isThemeModalOpen,
		canInitializeGitRepo,
		stagedFileCount,
		visibleFilePaths,
		selectedVisibleIndex,
		selectedFile,
		onIntent: onKeyboardIntent,
	});

	const selectedFileDiffState = useMemo(() => {
		if (!selectedFile) {
			return {
				loading: false,
				diff: "",
				note: Option.none<string>(),
			};
		}
		if (
			!selectedFilePreview ||
			selectedFilePreview.path !== selectedFile.path ||
			selectedFilePreview.status !== selectedFile.status
		) {
			return {
				loading: true,
				diff: "",
				note: Option.none<string>(),
			};
		}
		return {
			loading: selectedFilePreview.loading,
			diff: selectedFilePreview.preview.diff,
			note: selectedFilePreview.preview.note,
		};
	}, [selectedFile, selectedFilePreview]);

	return (
		<AppContent
			theme={theme}
			uiShowSplash={uiStatus.showSplash}
			uiError={uiStatus.error}
			isCommitModalOpen={isCommitModalOpen}
			isDiscardModalOpen={isDiscardModalOpen}
			isHelpModalOpen={isHelpModalOpen}
			isThemeModalOpen={isThemeModalOpen}
			modalBackdropColor={modalBackdropColor}
			commitMessage={commitMessage}
			commitError={commitError}
			files={files}
			sidebarItems={sidebarItems}
			remoteSync={remoteSync}
			selectedFile={selectedFile}
			selectedFileDiff={selectedFileDiffState.diff}
			selectedFileDiffNote={selectedFileDiffState.note}
			selectedFileDiffLoading={selectedFileDiffState.loading}
			loading={loading}
			diffViewMode={diffViewMode}
			syntaxStyle={themeBundle.syntaxStyle}
			diffScrollRef={diffScrollRef}
			onToggleDirectory={onToggleDirectory}
			onSelectFilePath={onSelectFilePath}
			sidebarOpen={sidebarOpen}
			onToggleSidebar={onToggleSidebar}
			onCommitMessageChange={onCommitMessageChange}
			onCommitSubmit={onCommitSubmit}
			snackbarNotice={snackbarNotice}
			onCopySelection={onCopySelection}
			themeNames={filteredThemeNames}
			selectedThemeName={selectedThemeName}
			themeSearchQuery={themeSearchQuery}
			onThemeSearchQueryChange={setThemeSearchQuery}
			onSelectThemeInModal={onSelectThemeInModal}
			discardModalFile={discardModalFile}
			onCancelDiscardModal={onCancelDiscardModal}
			onConfirmDiscardModal={onConfirmDiscardModal}
		/>
	);
}
