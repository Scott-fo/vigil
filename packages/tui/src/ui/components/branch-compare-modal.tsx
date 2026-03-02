import { RGBA, type ScrollBoxRenderable } from "@opentui/core";
import { Option } from "effect";
import { memo, useRef } from "react";
import type { ResolvedTheme } from "#theme/theme";
import type { BranchCompareField } from "#ui/state";
import { useScrollFollowSelection } from "#ui/hooks/use-scroll-follow-selection";

export interface BranchCompareModalProps {
	readonly theme: ResolvedTheme;
	readonly modalBackdropColor: RGBA;
	readonly sourceQuery: string;
	readonly destinationQuery: string;
	readonly sourceRef: Option.Option<string>;
	readonly destinationRef: Option.Option<string>;
	readonly activeField: BranchCompareField;
	readonly filteredRefs: ReadonlyArray<string>;
	readonly selectedActiveRef: Option.Option<string>;
	readonly loading: boolean;
	readonly error: Option.Option<string>;
	readonly onSourceQueryChange: (value: string) => void;
	readonly onDestinationQueryChange: (value: string) => void;
	readonly onSelectRef: (refName: string) => void;
	readonly onActivateField: (field: BranchCompareField) => void;
}

export const BranchCompareModal = memo(function BranchCompareModal(
	props: BranchCompareModalProps,
) {
	const refsScrollRef = useRef<ScrollBoxRenderable | null>(null);
	const selectedRowId = Option.match(props.selectedActiveRef, {
		onNone: () => null,
		onSome: (refName) => `branch-ref:${refName}`,
	});

	useScrollFollowSelection({
		scrollRef: refsScrollRef,
		selectedRowId,
	});

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
			zIndex={130}
		>
			<box
				width={82}
				height={24}
				padding={1}
				gap={1}
				border
				borderStyle="rounded"
				borderColor={props.theme.borderActive}
				backgroundColor={props.theme.backgroundPanel}
				flexDirection="column"
			>
				<text fg={props.theme.text}>
					<strong>Branch Compare</strong>
				</text>
				<text fg={props.theme.textMuted}>
					Pick source and destination refs. Enter applies comparison.
				</text>
				<box
					paddingX={1}
					flexDirection="column"
					width="100%"
					gap={1}
					onMouseDown={(event) => {
						event.preventDefault();
						props.onActivateField("source");
					}}
				>
					<text
						fg={
							props.activeField === "source"
								? props.theme.primary
								: props.theme.textMuted
						}
					>
						Source
					</text>
					<input
						value={props.sourceQuery}
						onInput={props.onSourceQueryChange}
						placeholder={Option.match(props.sourceRef, {
							onNone: () => "Type to filter refs...",
							onSome: (refName) => refName,
						})}
						focused={props.activeField === "source"}
						width="100%"
						backgroundColor={props.theme.backgroundElement}
						focusedBackgroundColor={props.theme.backgroundElement}
						textColor={props.theme.text}
						focusedTextColor={props.theme.text}
						placeholderColor={props.theme.textMuted}
					/>
				</box>
				<box
					paddingX={1}
					flexDirection="column"
					width="100%"
					gap={1}
					onMouseDown={(event) => {
						event.preventDefault();
						props.onActivateField("destination");
					}}
				>
					<text
						fg={
							props.activeField === "destination"
								? props.theme.primary
								: props.theme.textMuted
						}
					>
						Destination
					</text>
					<input
						value={props.destinationQuery}
						onInput={props.onDestinationQueryChange}
						placeholder={Option.match(props.destinationRef, {
							onNone: () => "Type to filter refs...",
							onSome: (refName) => refName,
						})}
						focused={props.activeField === "destination"}
						width="100%"
						backgroundColor={props.theme.backgroundElement}
						focusedBackgroundColor={props.theme.backgroundElement}
						textColor={props.theme.text}
						focusedTextColor={props.theme.text}
						placeholderColor={props.theme.textMuted}
					/>
				</box>
				<box flexGrow={1} border borderStyle="rounded" borderColor={props.theme.border}>
					<scrollbox ref={refsScrollRef} flexGrow={1}>
						{props.loading ? (
							<box paddingX={1}>
								<text fg={props.theme.textMuted}>Loading refs...</text>
							</box>
						) : props.filteredRefs.length === 0 ? (
							<box paddingX={1}>
								<text fg={props.theme.textMuted}>No matching refs.</text>
							</box>
						) : (
							props.filteredRefs.map((refName) => {
								const selected = Option.match(props.selectedActiveRef, {
									onNone: () => false,
									onSome: (selectedRef) => selectedRef === refName,
								});
								return (
									<box
										key={refName}
										id={`branch-ref:${refName}`}
										paddingX={1}
										backgroundColor={
											selected ? props.theme.primary : "transparent"
										}
										onMouseDown={(event) => {
											event.preventDefault();
											props.onSelectRef(refName);
										}}
									>
										<text
											fg={
												selected
													? props.theme.selectedListItemText
													: props.theme.text
											}
										>
											{refName}
										</text>
									</box>
								);
							})
						)}
					</scrollbox>
				</box>
				{Option.isSome(props.error) ? (
					<text fg={props.theme.error}>{props.error.value}</text>
				) : (
					<text fg={props.theme.textMuted}>
						Tab switches field. Up/Down selects ref. Esc cancels.
					</text>
				)}
			</box>
		</box>
	);
});
