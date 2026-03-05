import type { RGBA, ScrollBoxRenderable } from "@opentui/core";
import { Option } from "effect";
import { memo, useEffect, useMemo, useRef, useState } from "react";
import type { CommitDiffSelection } from "#data/git.ts";
import type { ResolvedTheme } from "#theme/theme.ts";
import {
	calculateSidebarVirtualWindow,
	getScrollTopForVisibleRow,
} from "#ui/sidebar-virtualization.ts";

export interface CommitSearchModalProps {
	readonly theme: ResolvedTheme;
	readonly modalBackdropColor: RGBA;
	readonly query: string;
	readonly commits: ReadonlyArray<CommitDiffSelection>;
	readonly selectedCommitHash: Option.Option<string>;
	readonly selectedIndex: number;
	readonly loading: boolean;
	readonly error: Option.Option<string>;
	readonly onQueryChange: (value: string) => void;
	readonly onSelectCommit: (commitHash: string) => void;
}

const COMMIT_OVERSCAN_ROWS = 120;
const SCROLL_POLL_MS = 33;

function getViewportHeight(scroll: ScrollBoxRenderable): number {
	return Math.max(1, Math.floor(scroll.viewport.height));
}

export const CommitSearchModal = memo(function CommitSearchModal(
	props: CommitSearchModalProps,
) {
	const commitsScrollRef = useRef<ScrollBoxRenderable | null>(null);
	const [scrollMetrics, setScrollMetrics] = useState({
		scrollTop: 0,
		viewportHeight: 1,
	});

	useEffect(() => {
		const scroll = commitsScrollRef.current;
		if (!scroll) {
			return;
		}

		const syncScrollMetrics = () => {
			const next = {
				scrollTop: Math.max(0, Math.floor(scroll.scrollTop)),
				viewportHeight: getViewportHeight(scroll),
			};
			setScrollMetrics((current) =>
				current.scrollTop === next.scrollTop &&
				current.viewportHeight === next.viewportHeight
					? current
					: next,
			);
		};

		syncScrollMetrics();
		const interval = setInterval(syncScrollMetrics, SCROLL_POLL_MS);
		interval.unref?.();
		return () => {
			clearInterval(interval);
		};
	}, [props.commits.length]);

	useEffect(() => {
		if (props.selectedIndex < 0) {
			return;
		}

		const scroll = commitsScrollRef.current;
		if (!scroll) {
			return;
		}

		const nextScrollTop = getScrollTopForVisibleRow(
			props.selectedIndex,
			scroll.scrollTop,
			getViewportHeight(scroll),
		);

		if (nextScrollTop !== Math.floor(scroll.scrollTop)) {
			scroll.scrollTo({ x: 0, y: nextScrollTop });
			setScrollMetrics((current) =>
				current.scrollTop === nextScrollTop &&
				current.viewportHeight === getViewportHeight(scroll)
					? current
					: {
							scrollTop: nextScrollTop,
							viewportHeight: getViewportHeight(scroll),
						},
			);
		}
	}, [props.selectedIndex]);

	const virtualWindow = useMemo(
		() =>
			calculateSidebarVirtualWindow({
				totalRows: props.commits.length,
				scrollTop: scrollMetrics.scrollTop,
				viewportHeight: scrollMetrics.viewportHeight,
				overscan: COMMIT_OVERSCAN_ROWS,
			}),
		[props.commits.length, scrollMetrics.scrollTop, scrollMetrics.viewportHeight],
	);

	const visibleCommits = useMemo(
		() => props.commits.slice(virtualWindow.start, virtualWindow.end),
		[props.commits, virtualWindow.end, virtualWindow.start],
	);

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
			zIndex={125}
		>
			<box
				width={94}
				height="90%"
				maxHeight={28}
				minHeight={12}
				padding={1}
				gap={1}
				border
				borderStyle="rounded"
				borderColor={props.theme.borderActive}
				backgroundColor={props.theme.backgroundPanel}
				flexDirection="column"
			>
				<text fg={props.theme.text}>
					<strong>Commit Search</strong>
				</text>
				<input
					value={props.query}
					onInput={props.onQueryChange}
					placeholder="Search by hash or subject..."
					focused
					width="100%"
					backgroundColor={props.theme.backgroundElement}
					focusedBackgroundColor={props.theme.backgroundElement}
					textColor={props.theme.text}
					focusedTextColor={props.theme.text}
					placeholderColor={props.theme.textMuted}
				/>
				<box
					flexGrow={1}
					minHeight={0}
					border
					borderStyle="rounded"
					borderColor={props.theme.border}
				>
					<scrollbox
						ref={commitsScrollRef}
						flexGrow={1}
						height="100%"
						viewportCulling
					>
						{props.loading ? (
							<box paddingX={1}>
								<text fg={props.theme.textMuted}>Loading commits...</text>
							</box>
						) : props.commits.length === 0 ? (
							<box paddingX={1}>
								<text fg={props.theme.textMuted}>No matching commits.</text>
							</box>
						) : (
							<>
								{virtualWindow.topPadding > 0 ? (
									<box height={virtualWindow.topPadding} />
								) : null}
								{visibleCommits.map((commit, index) => {
									const commitIndex = virtualWindow.start + index;
									const selected =
										commitIndex === props.selectedIndex ||
										(Option.isSome(props.selectedCommitHash) &&
											props.selectedCommitHash.value === commit.commitHash);
									return (
										<box
											key={commit.commitHash}
											id={`commit-row:${commit.commitHash}`}
											paddingX={1}
											backgroundColor={
												selected ? props.theme.primary : "transparent"
											}
											onMouseDown={(event) => {
												event.preventDefault();
												props.onSelectCommit(commit.commitHash);
											}}
										>
											<text
												wrapMode="none"
												truncate
												fg={
													selected
														? props.theme.selectedListItemText
														: props.theme.text
												}
											>
												<span
													fg={
														selected
															? props.theme.selectedListItemText
															: props.theme.accent
													}
												>
													{commit.shortHash}
												</span>{" "}
												{commit.subject}
											</text>
										</box>
									);
								})}
								{virtualWindow.bottomPadding > 0 ? (
									<box height={virtualWindow.bottomPadding} />
								) : null}
							</>
						)}
					</scrollbox>
				</box>
				{Option.isSome(props.error) ? (
					<text fg={props.theme.error}>{props.error.value}</text>
				) : (
					<text fg={props.theme.textMuted}>
						G opens this search. Up/Down selects. Enter applies. Esc cancels.
					</text>
				)}
			</box>
		</box>
	);
});
