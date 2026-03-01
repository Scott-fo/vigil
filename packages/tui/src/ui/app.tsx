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
import { isFileStaged, type RepoActionError } from "#data/git";
import { type ResolvedTheme, resolveThemeBundle, type ThemeMode } from "#theme/theme";
import type { AppProps, FileEntry } from "#tui/types";
import { CommitModal } from "#ui/components/commit-modal";
import { HelpModal } from "#ui/components/help-modal";
import { Reviewer } from "#ui/components/reviewer";
import { Snackbar, type SnackbarNotice } from "#ui/components/snackbar";
import { Splash } from "#ui/components/splash";
import { useFileRefresh } from "#ui/hooks/use-file-refresh";
import { useRepoActions } from "#ui/hooks/use-repo-actions";
import { useAppKeyboardInput } from "#ui/inputs";
import { buildSidebarItems, type SidebarItem } from "#ui/sidebar";
import {
	commitModalAtom,
	fileViewStateAtom,
	helpModalAtom,
	type UpdateCommitModal,
	type UpdateFileViewState,
	type UpdateHelpModal,
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
	readonly isHelpModalOpen: boolean;
	readonly modalBackdropColor: RGBA;
	readonly commitMessage: string;
	readonly commitError: Option.Option<string>;
	readonly files: FileEntry[];
	readonly sidebarItems: SidebarItem[];
	readonly selectedFile: FileEntry | null;
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
				<Splash theme={props.theme} />
			) : (
				<>
					<Reviewer
						theme={props.theme}
						syntaxStyle={props.syntaxStyle}
						files={props.files}
						sidebarItems={props.sidebarItems}
						selectedFile={props.selectedFile}
						loading={props.loading}
						diffViewMode={props.diffViewMode}
						error={props.uiError}
						isCommitModalOpen={
							props.isCommitModalOpen || props.isHelpModalOpen
						}
						diffScrollRef={props.diffScrollRef}
						onToggleDirectory={props.onToggleDirectory}
						onSelectFilePath={props.onSelectFilePath}
						sidebarOpen={props.sidebarOpen}
						onToggleSidebar={props.onToggleSidebar}
						onCopySelection={props.onCopySelection}
					/>
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
					{props.isHelpModalOpen && (
						<HelpModal
							theme={props.theme}
							modalBackdropColor={props.modalBackdropColor}
						/>
					)}
					<Snackbar theme={props.theme} notice={props.snackbarNotice} />
				</>
			)}
		</box>
	);
}

export function App(props: AppProps) {
	const renderer = useRenderer();
	const [themeName, setThemeName] = useState(props.initialThemeName);
	const [themeMode] = useState<ThemeMode>(props.initialThemeMode);
	const [fileView, setFileView] = useAtom(fileViewStateAtom);
	const [uiStatus, setUiStatus] = useAtom(uiStatusAtom);
	const [commitModal, setCommitModal] = useAtom(commitModalAtom);
	const [helpModal, setHelpModal] = useAtom(helpModalAtom);
	const [snackbarNotice, setSnackbarNotice] = useState<
		Option.Option<SnackbarNotice>
	>(Option.none());
	const diffScrollRef = useRef<ScrollBoxRenderable | null>(null);
	const snackbarTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

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
	const isHelpModalOpen = helpModal.isOpen;
	const commitMessage = commitModal.isOpen ? commitModal.message : "";
	const commitError = commitModal.isOpen ? commitModal.error : Option.none();

	const { refreshFiles } = useFileRefresh({
		updateFileView,
		updateUiStatus,
		renderRepoActionError: formatRepoActionError,
		pollMs: 2000,
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
		onKeyboardIntent,
		onToggleDirectory,
		onSelectFilePath,
		onToggleSidebar,
	} = useRepoActions({
		chooserFilePath: props.chooserFilePath,
		renderer,
		diffScrollRef,
		themeCatalog: props.themeCatalog,
		setThemeName,
		stagedFileCount,
		commitModal,
		helpModal,
		updateFileView,
		updateUiStatus,
		updateCommitModal,
		updateHelpModal,
		refreshFiles,
		renderRepoActionError: formatRepoActionError,
	});

	useAppKeyboardInput({
		isCommitModalOpen,
		isHelpModalOpen,
		stagedFileCount,
		visibleFilePaths,
		selectedVisibleIndex,
		selectedFile,
		onIntent: onKeyboardIntent,
	});

	return (
		<AppContent
			theme={theme}
			uiShowSplash={uiStatus.showSplash}
			uiError={uiStatus.error}
			isCommitModalOpen={isCommitModalOpen}
			isHelpModalOpen={isHelpModalOpen}
			modalBackdropColor={modalBackdropColor}
			commitMessage={commitMessage}
			commitError={commitError}
			files={files}
			sidebarItems={sidebarItems}
			selectedFile={selectedFile}
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
		/>
	);
}
