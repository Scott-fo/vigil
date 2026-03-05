import { RGBA } from "@opentui/core";
import { Option, pipe } from "effect";
import { useMemo } from "react";
import { isFileStaged } from "#data/git.ts";
import {
	resolveThemeBundle,
	type ThemeCatalog,
	type ThemeMode,
} from "#theme/theme.ts";
import type { SidebarItem } from "#ui/sidebar.ts";
import { buildSidebarTree, flattenSidebarTree } from "#ui/sidebar.ts";
import { searchBranchRefs } from "#ui/branch-ref-search.ts";
import { searchCommits } from "#ui/commit-search.ts";
import type {
	BranchCompareField,
	BranchCompareModalState,
	CommitSearchModalState,
	CommitModalState,
	DiscardModalState,
	FileViewState,
	HelpModalState,
	ReviewMode,
	ThemeModalState,
	UiStatus,
} from "#ui/state.ts";
import {
	deriveModalVisibility,
	isCommitCompareReviewMode,
	isWorkingTreeReviewMode,
} from "#ui/state.ts";

interface UseAppSelectorsOptions {
	readonly fileView: FileViewState;
	readonly uiStatus: UiStatus;
	readonly commitModal: CommitModalState;
	readonly discardModal: DiscardModalState;
	readonly commitSearchModal: CommitSearchModalState;
	readonly helpModal: HelpModalState;
	readonly themeModal: ThemeModalState;
	readonly branchCompareModal: BranchCompareModalState;
	readonly reviewMode: ReviewMode;
	readonly themeCatalog: ThemeCatalog;
	readonly themeName: string;
	readonly themeMode: ThemeMode;
	readonly themeSearchQuery: string;
}

export function useAppSelectors(options: UseAppSelectorsOptions) {
	const {
		fileView,
		uiStatus,
		commitModal,
		discardModal,
		commitSearchModal,
		helpModal,
		themeModal,
		branchCompareModal,
		reviewMode,
		themeCatalog,
		themeName,
		themeMode,
		themeSearchQuery,
	} = options;

	const {
		files,
		sidebarOpen,
		diffViewMode,
		collapsedDirectories,
		selectedPath,
		loading,
	} = fileView;

	const themeBundle = useMemo(
		() => resolveThemeBundle(themeCatalog, themeName, themeMode),
		[themeCatalog, themeName, themeMode],
	);

	const theme = themeBundle.theme;

	const modalBackdropColor = RGBA.fromValues(
		theme.background.r,
		theme.background.g,
		theme.background.b,
		0.55,
	);

	const {
		isCommitModalOpen,
		isDiscardModalOpen,
		isCommitSearchModalOpen,
		isHelpModalOpen,
		isThemeModalOpen,
		isBranchCompareModalOpen,
		isAnyModalOpen,
	} = deriveModalVisibility({
		commitModal,
		discardModal,
		commitSearchModal,
		helpModal,
		themeModal,
		branchCompareModal,
	});
	const discardModalFile = discardModal.isOpen ? discardModal.file : null;

	const selectedThemeName = themeModal.isOpen
		? themeModal.selectedThemeName
		: themeName;

	const fileByPath = useMemo(
		() => new Map(files.map((file) => [file.path, file] as const)),
		[files],
	);

	const branchSourceQuery = branchCompareModal.isOpen
		? branchCompareModal.sourceQuery
		: "";

	const branchDestinationQuery = branchCompareModal.isOpen
		? branchCompareModal.destinationQuery
		: "";

	const branchSourceRef = branchCompareModal.isOpen
		? branchCompareModal.sourceRef
		: Option.none<string>();

	const branchDestinationRef = branchCompareModal.isOpen
		? branchCompareModal.destinationRef
		: Option.none<string>();

	const branchActiveField: BranchCompareField = branchCompareModal.isOpen
		? branchCompareModal.activeField
		: "source";

	const branchFilteredRefs = useMemo(() => {
		if (!branchCompareModal.isOpen) {
			return [] as const;
		}

		const query =
			branchCompareModal.activeField === "source"
				? branchCompareModal.sourceQuery
				: branchCompareModal.destinationQuery;

		return searchBranchRefs(branchCompareModal.availableRefs, query);
	}, [branchCompareModal]);

	const branchSelectedActiveRef = useMemo(() => {
		if (!branchCompareModal.isOpen) {
			return Option.none<string>();
		}

		const selectedIndex =
			branchCompareModal.activeField === "source"
				? branchCompareModal.selectedSourceIndex
				: branchCompareModal.selectedDestinationIndex;

		const selectedByIndex = branchFilteredRefs[selectedIndex];
		if (selectedByIndex) {
			return Option.some(selectedByIndex);
		}

		const selectedRef =
			branchCompareModal.activeField === "source"
				? branchCompareModal.sourceRef
				: branchCompareModal.destinationRef;

		return Option.isSome(selectedRef) &&
			branchFilteredRefs.includes(selectedRef.value)
			? selectedRef
			: Option.fromNullable(branchFilteredRefs[0]);
	}, [branchCompareModal, branchFilteredRefs]);

	const branchModalLoading = branchCompareModal.isOpen
		? branchCompareModal.loading
		: false;

	const branchModalError = branchCompareModal.isOpen
		? branchCompareModal.error
		: Option.none<string>();

	const filteredThemeNames = useMemo(() => {
		const query = themeSearchQuery.trim().toLowerCase();
		if (query.length === 0) {
			return themeCatalog.order;
		}
		return themeCatalog.order.filter((themeCatalogName) =>
			themeCatalogName.toLowerCase().includes(query),
		);
	}, [themeCatalog.order, themeSearchQuery]);

	const commitMessage = commitModal.isOpen ? commitModal.message : "";
	const commitError = commitModal.isOpen ? commitModal.error : Option.none();

	const commitSearchQuery = commitSearchModal.isOpen ? commitSearchModal.query : "";

	const commitFilteredCommits = useMemo(() => {
		if (!commitSearchModal.isOpen) {
			return [] as const;
		}
		return searchCommits(
			commitSearchModal.availableCommits,
			commitSearchModal.query,
		);
	}, [commitSearchModal]);

	const commitSelectedIndex = useMemo(() => {
		if (!commitSearchModal.isOpen) {
			return 0;
		}
		const maxIndex = Math.max(commitFilteredCommits.length - 1, 0);
		return Math.min(Math.max(commitSearchModal.selectedIndex, 0), maxIndex);
	}, [commitFilteredCommits.length, commitSearchModal]);

	const commitSelectedCommitHash = useMemo(() => {
		if (!commitSearchModal.isOpen) {
			return Option.none<string>();
		}

		const selectedByIndex = commitFilteredCommits[commitSelectedIndex];
		if (selectedByIndex) {
			return Option.some(selectedByIndex.commitHash);
		}

		const selectedCommitHashValue = Option.match(
			commitSearchModal.selectedCommitHash,
			{
				onNone: () => null,
				onSome: (value) => value,
			},
		);

		return selectedCommitHashValue !== null &&
			commitFilteredCommits.some(
				(commit) => commit.commitHash === selectedCommitHashValue,
			)
			? commitSearchModal.selectedCommitHash
			: Option.fromNullable(commitFilteredCommits[0]?.commitHash);
	}, [commitFilteredCommits, commitSearchModal, commitSelectedIndex]);

	const commitSearchModalLoading = commitSearchModal.isOpen
		? commitSearchModal.loading
		: false;

	const commitSearchModalError = commitSearchModal.isOpen
		? commitSearchModal.error
		: Option.none<string>();

	const canInitializeGitRepo = pipe(
		uiStatus.error,
		Option.match({
			onNone: () => false,
			onSome: (error) =>
				uiStatus.showSplash && /not a git repository/i.test(error),
		}),
	);

	const reviewModeLabel = isWorkingTreeReviewMode(reviewMode)
		? ""
		: isCommitCompareReviewMode(reviewMode)
			? `Commit ${reviewMode.selection.shortHash}: ${reviewMode.selection.subject}`
			: `Compare ${reviewMode.selection.sourceRef} -> ${reviewMode.selection.destinationRef}`;

	const selectedFile = useMemo(() => {
		if (files.length === 0) {
			return null;
		}

		const selectedFileMatch = pipe(
			selectedPath,
			Option.flatMap((path) => Option.fromNullable(fileByPath.get(path))),
		);

		return pipe(
			selectedFileMatch,
			Option.getOrElse(() => files[0] ?? null),
		);
	}, [fileByPath, files.length, selectedPath]);

	const sidebarTree = useMemo(() => buildSidebarTree(files), [files]);

	const sidebarItems = useMemo(
		() => flattenSidebarTree(sidebarTree, collapsedDirectories),
		[collapsedDirectories, sidebarTree],
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

	const visibleFileIndexByPath = useMemo(
		() =>
			new Map(visibleFilePaths.map((path, index) => [path, index] as const)),
		[visibleFilePaths],
	);

	const selectedVisibleIndex = useMemo(() => {
		if (!selectedFile) {
			return -1;
		}

		return visibleFileIndexByPath.get(selectedFile.path) ?? -1;
	}, [selectedFile, visibleFileIndexByPath]);

	const stagedFileCount = useMemo(
		() => files.filter((file) => isFileStaged(file.status)).length,
		[files],
	);

	return {
		files,
		sidebarOpen,
		diffViewMode,
		loading,
		themeBundle,
		theme,
		modalBackdropColor,
		isCommitModalOpen,
		isDiscardModalOpen,
		isCommitSearchModalOpen,
		isHelpModalOpen,
		isThemeModalOpen,
		isBranchCompareModalOpen,
		isAnyModalOpen,
		discardModalFile,
		selectedThemeName,
		branchSourceQuery,
		branchDestinationQuery,
		branchSourceRef,
		branchDestinationRef,
		branchActiveField,
		branchFilteredRefs,
		branchSelectedActiveRef,
		branchModalLoading,
		branchModalError,
		filteredThemeNames,
		commitMessage,
		commitError,
		commitSearchQuery,
		commitFilteredCommits,
		commitSelectedCommitHash,
		commitSelectedIndex,
		commitSearchModalLoading,
		commitSearchModalError,
		canInitializeGitRepo,
		reviewModeLabel,
		selectedFile,
		sidebarItems,
		visibleFilePaths,
		selectedVisibleIndex,
		stagedFileCount,
	};
}
