/** biome-ignore-all lint/a11y/noStaticElementInteractions: <opentui> */

import { useAtom } from "@effect-atom/atom-react";
import {
	RGBA,
	type ScrollBoxRenderable,
	type SyntaxStyle,
} from "@opentui/core";
import { useRenderer } from "@opentui/react";
import { Effect, Option, pipe } from "effect";
import {
	type RefObject,
	useCallback,
	useEffect,
	useMemo,
	useRef,
	useState,
} from "react";
import { isFileStaged, type RepoActionError } from "#data/git";
import { type ResolvedTheme, resolveThemeBundle, type ThemeMode } from "#theme/theme";
import type { AppProps, FileEntry } from "#tui/types";
import { CommitModal } from "#ui/components/commit-modal";
import { Reviewer } from "#ui/components/reviewer";
import { Splash } from "#ui/components/splash";
import { useFileRefresh } from "#ui/hooks/use-file-refresh";
import { useRepoActions } from "#ui/hooks/use-repo-actions";
import { useAppKeyboardInput } from "#ui/inputs";
import { buildSidebarItems, type SidebarItem } from "#ui/sidebar";
import {
	commitModalAtom,
	fileViewStateAtom,
	type UpdateCommitModal,
	type UpdateFileViewState,
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
						isCommitModalOpen={props.isCommitModalOpen}
						diffScrollRef={props.diffScrollRef}
						onToggleDirectory={props.onToggleDirectory}
						onSelectFilePath={props.onSelectFilePath}
						sidebarOpen={props.sidebarOpen}
						onToggleSidebar={props.onToggleSidebar}
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
	const diffScrollRef = useRef<ScrollBoxRenderable | null>(null);

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
		updateFileView,
		updateUiStatus,
		updateCommitModal,
		refreshFiles,
		renderRepoActionError: formatRepoActionError,
	});

	useAppKeyboardInput({
		isCommitModalOpen,
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
		/>
	);
}
