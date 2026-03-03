import type {
	DiffRenderable,
	LineColorConfig,
	ScrollBoxRenderable,
	SyntaxStyle,
} from "@opentui/core";
import { Option, pipe } from "effect";
import { memo, type RefObject, useEffect, useMemo, useRef } from "react";
import { isFileStaged } from "#data/git";
import { splitDiffIntoHunkBlocks } from "#diff/hunks";
import type { DiffNavigationLine } from "#diff/navigation";
import type { ResolvedTheme } from "#theme/theme";
import type { FileEntry } from "#tui/types";
import { useScrollFollowSelection } from "#ui/hooks/use-scroll-follow-selection";
import type { FocusedPane } from "#ui/inputs";
import { getStatusColor, type SidebarItem } from "#ui/sidebar";

type SidebarHeaderItem = Extract<SidebarItem, { kind: "header" }>;
type SidebarFileItem = Extract<SidebarItem, { kind: "file" }>;

function getDiffLineBaseColor(
	line: DiffNavigationLine,
	theme: ResolvedTheme,
): LineColorConfig {
	if (line.kind === "add") {
		return {
			gutter: theme.diffAddedLineNumberBg,
			content: theme.diffAddedBg,
		};
	}

	if (line.kind === "remove") {
		return {
			gutter: theme.diffRemovedLineNumberBg,
			content: theme.diffRemovedBg,
		};
	}

	return {
		gutter: theme.diffContextBg,
		content: theme.diffContextBg,
	};
}

function getSelectedDiffLineColor(theme: ResolvedTheme): LineColorConfig {
	return {
		gutter: theme.primary,
		content: theme.backgroundElement,
	};
}

function ensureDiffLineVisible(
	scrollBox: ScrollBoxRenderable,
	lineIndex: number,
): void {
	const viewportHeight = Math.max(1, scrollBox.height);
	const scrollTop = scrollBox.scrollTop;
	const viewportBottom = scrollTop + viewportHeight - 1;

	if (lineIndex < scrollTop) {
		scrollBox.scrollTo({ x: 0, y: lineIndex });
		return;
	}

	if (lineIndex > viewportBottom) {
		scrollBox.scrollTo({ x: 0, y: lineIndex - viewportHeight + 1 });
	}
}

interface SidebarHeaderRowProps {
	readonly item: SidebarHeaderItem;
	readonly theme: ResolvedTheme;
	readonly onToggleDirectory: (path: string) => void;
}

const SidebarHeaderRow = memo(function SidebarHeaderRow(
	props: SidebarHeaderRowProps,
) {
	const headerPrefix = "  ".repeat(props.item.depth);
	return (
		<box
			paddingX={1}
			onMouseDown={(event) => {
				event.preventDefault();
				props.onToggleDirectory(props.item.path);
			}}
		>
			<text fg={props.theme.textMuted}>
				<span fg={props.theme.borderActive}>
					{headerPrefix}
					{props.item.collapsed ? "▸ " : "▾ "}
				</span>
				{props.item.label}
			</text>
		</box>
	);
});

interface SidebarFileRowProps {
	readonly item: SidebarFileItem;
	readonly rowId: string;
	readonly theme: ResolvedTheme;
	readonly selectedFilePath: Option.Option<string>;
	readonly onSelectFilePath: (path: string) => void;
}

const SidebarFileRow = memo(function SidebarFileRow(
	props: SidebarFileRowProps,
) {
	const { file } = props.item;

	const selected =
		Option.isSome(props.selectedFilePath) &&
		props.selectedFilePath.value === file.path;

	const staged = isFileStaged(file.status);

	const rowBackground = selected
		? staged
			? props.theme.diffAddedLineNumberBg
			: props.theme.backgroundElement
		: staged
			? props.theme.diffAddedBg
			: "transparent";

	const filePrefix = "  ".repeat(props.item.depth);

	return (
		<box
			id={props.rowId}
			paddingX={1}
			backgroundColor={rowBackground}
			onMouseDown={(event) => {
				event.preventDefault();
				props.onSelectFilePath(file.path);
			}}
		>
			<text>
				<span fg={props.theme.borderSubtle}>{filePrefix}</span>
				<span fg={getStatusColor(file.status, props.theme)}>{file.status}</span>{" "}
				<span
					fg={selected || staged ? props.theme.text : props.theme.textMuted}
				>
					{props.item.label}
				</span>
			</text>
		</box>
	);
});

interface SidebarPanelProps {
	readonly theme: ResolvedTheme;
	readonly files: FileEntry[];
	readonly sidebarItems: SidebarItem[];
	readonly isFocused: boolean;
	readonly selectedFilePath: Option.Option<string>;
	readonly onFocus: () => void;
	readonly onToggleDirectory: (path: string) => void;
	readonly onSelectFilePath: (path: string) => void;
}

const SidebarPanel = memo(function SidebarPanel(props: SidebarPanelProps) {
	const sidebarScrollRef = useRef<ScrollBoxRenderable | null>(null);
	const selectedRowId = pipe(
		props.selectedFilePath,
		Option.map((selectedPath) => `sidebar-row:${selectedPath}`),
		Option.getOrElse(() => null),
	);

	useScrollFollowSelection({
		scrollRef: sidebarScrollRef,
		selectedRowId,
	});

	return (
		<box
			width={38}
			border
			borderStyle="rounded"
			borderColor={
				props.isFocused ? props.theme.borderActive : props.theme.border
			}
			marginRight={1}
			flexDirection="column"
			backgroundColor={props.theme.backgroundPanel}
			onMouseDown={() => {
				props.onFocus();
			}}
		>
			<box paddingX={1} marginBottom={1}>
				<text fg={props.theme.text}>
					<strong>Changed Files ({props.files.length})</strong>
				</text>
			</box>

			<scrollbox ref={sidebarScrollRef} flexGrow={1} focused={props.isFocused}>
				{props.sidebarItems.map((item) =>
					item.kind === "header" ? (
						<SidebarHeaderRow
							key={item.key}
							item={item}
							theme={props.theme}
							onToggleDirectory={props.onToggleDirectory}
						/>
					) : (
						<SidebarFileRow
							key={item.key}
							item={item}
							rowId={`sidebar-row:${item.file.path}`}
							theme={props.theme}
							selectedFilePath={props.selectedFilePath}
							onSelectFilePath={props.onSelectFilePath}
						/>
					),
				)}
			</scrollbox>
		</box>
	);
});

interface SidebarRailProps {
	readonly theme: ResolvedTheme;
	readonly onToggleSidebar: () => void;
}

const SidebarRail = memo(function SidebarRail(props: SidebarRailProps) {
	return (
		<box
			width={3}
			border
			borderStyle="rounded"
			borderColor={props.theme.border}
			marginRight={1}
			justifyContent="center"
			alignItems="center"
			backgroundColor={props.theme.backgroundPanel}
			onMouseDown={(event) => {
				event.preventDefault();
				props.onToggleSidebar();
			}}
		>
			<text fg={props.theme.textMuted}>▸</text>
		</box>
	);
});

interface DiffPanelProps {
	readonly theme: ResolvedTheme;
	readonly syntaxStyle: SyntaxStyle;
	readonly reviewModeLabel: string;
	readonly diffViewMode: "split" | "unified";
	readonly selectedDiffLineIndex: number;
	readonly diffNavigationLines: ReadonlyArray<DiffNavigationLine>;
	readonly isFocused: boolean;
	readonly selectedFile: FileEntry | null;
	readonly selectedFileDiff: string;
	readonly selectedFileDiffNote: Option.Option<string>;
	readonly selectedFileDiffLoading: boolean;
	readonly loading: boolean;
	readonly error: Option.Option<string>;
	readonly isCommitModalOpen: boolean;
	readonly diffScrollRef: RefObject<ScrollBoxRenderable | null>;
	readonly onFocus: () => void;
}

const DiffPanel = memo(function DiffPanel(props: DiffPanelProps) {
	const diffRef = useRef<DiffRenderable | null>(null);
	const highlightedLineRef = useRef<DiffNavigationLine | null>(null);

	const effectiveDiffViewMode: "split" | "unified" = props.isFocused
		? "unified"
		: props.diffViewMode;

	const selectedFileDiffBlocks = useMemo(() => {
		if (!props.selectedFileDiff.trim()) {
			return [];
		}
		return props.isFocused
			? [props.selectedFileDiff]
			: splitDiffIntoHunkBlocks(props.selectedFileDiff);
	}, [props.isFocused, props.selectedFileDiff]);

	useEffect(() => {
		highlightedLineRef.current = null;
	}, [props.selectedFile?.path, props.selectedFileDiff]);

	useEffect(() => {
		if (!props.isFocused) {
			highlightedLineRef.current = null;
			return;
		}

		const selectedLine = props.diffNavigationLines[props.selectedDiffLineIndex];
		if (!selectedLine) {
			highlightedLineRef.current = null;
			return;
		}

		const diffRenderable = diffRef.current;
		if (!diffRenderable) {
			return;
		}

		const previousHighlightedLine = highlightedLineRef.current;
		if (
			previousHighlightedLine &&
			previousHighlightedLine.displayIndex !== selectedLine.displayIndex
		) {
			diffRenderable.setLineColor(
				previousHighlightedLine.displayIndex,
				getDiffLineBaseColor(previousHighlightedLine, props.theme),
			);
		}

		diffRenderable.setLineColor(
			selectedLine.displayIndex,
			getSelectedDiffLineColor(props.theme),
		);

		const diffScroll = props.diffScrollRef.current;
		if (diffScroll) {
			ensureDiffLineVisible(diffScroll, selectedLine.displayIndex);
		}

		highlightedLineRef.current = selectedLine;
	}, [
		props.diffNavigationLines,
		props.diffScrollRef,
		props.isFocused,
		props.selectedDiffLineIndex,
		props.theme,
	]);

	return (
		<box
			flexGrow={1}
			border
			borderStyle="rounded"
			borderColor={
				props.isFocused ? props.theme.borderActive : props.theme.border
			}
			flexDirection="column"
			backgroundColor={props.theme.backgroundPanel}
			onMouseDown={() => {
				props.onFocus();
			}}
		>
			<box paddingX={1} marginBottom={1} flexDirection="column" width="100%">
				{props.reviewModeLabel.length > 0 ? (
					<box
						width="100%"
						height={1}
						justifyContent="flex-end"
						backgroundColor={props.theme.backgroundPanel}
					>
						<text fg={props.theme.textMuted}>{props.reviewModeLabel}</text>
					</box>
				) : null}
				<box
					width="100%"
					height={1}
					backgroundColor={props.theme.backgroundPanel}
				>
					<text fg={props.theme.text}>
						<strong>
							{props.selectedFile
								? props.selectedFile.label
								: "No file selected"}
						</strong>
					</text>
				</box>
			</box>

			<box flexGrow={1} padding={1}>
				{props.loading ? (
					<text fg={props.theme.textMuted}>Loading git status...</text>
				) : Option.isSome(props.error) ? (
					<text fg={props.theme.error}>{props.error.value}</text>
				) : !props.selectedFile ? (
					<text fg={props.theme.textMuted}>No changed files found.</text>
				) : props.selectedFileDiffLoading ? (
					<text fg={props.theme.textMuted}>Loading diff...</text>
				) : props.selectedFileDiff.trim() ? (
					<scrollbox
						ref={props.diffScrollRef}
						flexGrow={1}
						focused={!props.isCommitModalOpen && props.isFocused}
						verticalScrollbarOptions={{
							trackOptions: {
								backgroundColor: props.theme.backgroundElement,
								foregroundColor: props.theme.borderActive,
							},
						}}
					>
						<box flexDirection="column">
							{selectedFileDiffBlocks.map((hunkDiff, hunkIndex) => (
								<box
									key={`${props.selectedFile?.path}:${hunkIndex}`}
									flexDirection="column"
								>
									<diff
										diff={hunkDiff}
										{...(props.isFocused && hunkIndex === 0
											? { ref: diffRef }
											: {})}
										{...(props.selectedFile?.filetype
											? { filetype: props.selectedFile.filetype }
											: {})}
										syntaxStyle={props.syntaxStyle}
										view={effectiveDiffViewMode}
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
									{hunkIndex < selectedFileDiffBlocks.length - 1 ? (
										<box height={1} backgroundColor={props.theme.background} />
									) : null}
								</box>
							))}
						</box>
					</scrollbox>
				) : (
					<text fg={props.theme.textMuted}>
						{pipe(
							props.selectedFileDiffNote,
							Option.getOrElse(() => "No diff preview available."),
						)}
					</text>
				)}
			</box>
		</box>
	);
});

export interface ReviewerProps {
	readonly theme: ResolvedTheme;
	readonly syntaxStyle: SyntaxStyle;
	readonly reviewModeLabel: string;
	readonly files: FileEntry[];
	readonly sidebarItems: SidebarItem[];
	readonly selectedFile: FileEntry | null;
	readonly selectedFileDiff: string;
	readonly selectedFileDiffNote: Option.Option<string>;
	readonly selectedFileDiffLoading: boolean;
	readonly selectedDiffLineIndex: number;
	readonly diffNavigationLines: ReadonlyArray<DiffNavigationLine>;
	readonly loading: boolean;
	readonly diffViewMode: "split" | "unified";
	readonly error: Option.Option<string>;
	readonly isCommitModalOpen: boolean;
	readonly diffScrollRef: RefObject<ScrollBoxRenderable | null>;
	readonly onToggleDirectory: (path: string) => void;
	readonly onSelectFilePath: (path: string) => void;
	readonly sidebarOpen: boolean;
	readonly activePane: FocusedPane;
	readonly onToggleSidebar: () => void;
	readonly onFocusSidebar: () => void;
	readonly onFocusDiff: () => void;
	readonly onCopySelection: () => void;
}

export const Reviewer = memo(function Reviewer(props: ReviewerProps) {
	const selectedFilePath = pipe(
		Option.fromNullable(props.selectedFile),
		Option.map((file) => file.path),
	);
	const isSidebarFocused = props.sidebarOpen && props.activePane === "sidebar";
	const isDiffFocused = !props.sidebarOpen || props.activePane === "diff";

	return (
		<box
			flexDirection="row"
			flexGrow={1}
			onMouseUp={() => {
				props.onCopySelection();
			}}
		>
			{props.sidebarOpen ? (
				<SidebarPanel
					theme={props.theme}
					files={props.files}
					sidebarItems={props.sidebarItems}
					isFocused={isSidebarFocused}
					selectedFilePath={selectedFilePath}
					onFocus={props.onFocusSidebar}
					onToggleDirectory={props.onToggleDirectory}
					onSelectFilePath={props.onSelectFilePath}
				/>
			) : (
				<SidebarRail
					theme={props.theme}
					onToggleSidebar={() => {
						props.onToggleSidebar();
						props.onFocusSidebar();
					}}
				/>
			)}
			<DiffPanel
				theme={props.theme}
				syntaxStyle={props.syntaxStyle}
				reviewModeLabel={props.reviewModeLabel}
				diffViewMode={props.diffViewMode}
				selectedDiffLineIndex={props.selectedDiffLineIndex}
				diffNavigationLines={props.diffNavigationLines}
				isFocused={isDiffFocused}
				selectedFile={props.selectedFile}
				selectedFileDiff={props.selectedFileDiff}
				selectedFileDiffNote={props.selectedFileDiffNote}
				selectedFileDiffLoading={props.selectedFileDiffLoading}
				loading={props.loading}
				error={props.error}
				isCommitModalOpen={props.isCommitModalOpen}
				diffScrollRef={props.diffScrollRef}
				onFocus={props.onFocusDiff}
			/>
		</box>
	);
});
