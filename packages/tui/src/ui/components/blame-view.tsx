import type { RGBA, ScrollBoxRenderable } from "@opentui/core";
import { Option, pipe } from "effect";
import { memo, type RefObject } from "react";
import type { BlameCommitDetails } from "#data/git.ts";
import type { ResolvedTheme } from "#theme/theme.ts";
import type { BlameTarget } from "#tui/types.ts";

export interface BlameViewProps {
	readonly theme: ResolvedTheme;
	readonly modalBackdropColor: RGBA;
	readonly target: BlameTarget;
	readonly loading: boolean;
	readonly details: Option.Option<BlameCommitDetails>;
	readonly error: Option.Option<string>;
	readonly scrollRef: RefObject<ScrollBoxRenderable | null>;
}

function renderHintText(
	loading: boolean,
	details: Option.Option<BlameCommitDetails>,
): string {
	if (loading) {
		return "Loading commit details...";
	}
	if (Option.isNone(details)) {
		return "Esc closes this view.";
	}
	return Option.isSome(details.value.compareSelection)
		? "O opens commit comparison. J/K or Up/Down scroll. Esc closes."
		: "No commit comparison available for this line. Esc closes.";
}

export const BlameView = memo(function BlameView(props: BlameViewProps) {
	const details = pipe(props.details, Option.getOrNull);
	const description =
		details && details.description.trim().length > 0
			? details.description
			: "No commit description.";

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
				width="92%"
				maxWidth={96}
				height="90%"
				maxHeight={30}
				minHeight={12}
				padding={1}
				border
				borderStyle="rounded"
				borderColor={props.theme.borderActive}
				backgroundColor={props.theme.backgroundPanel}
				flexDirection="column"
			>
				<box marginBottom={1}>
					<text fg={props.theme.text}>
						<strong>
							Blame {props.target.filePath}:{props.target.lineNumber}
						</strong>
					</text>
				</box>

				{props.loading ? (
					<box marginBottom={1}>
						<text fg={props.theme.textMuted}>Loading blamed commit...</text>
					</box>
				) : Option.isSome(props.error) ? (
					<box marginBottom={1}>
						<text fg={props.theme.error}>{props.error.value}</text>
					</box>
				) : details ? (
					<>
						<box flexDirection="column" marginBottom={1}>
							<text fg={props.theme.text}>
								<strong>{details.subject || "(no commit title)"}</strong>
							</text>
							<box flexDirection="row" height={1}>
								<text fg={props.theme.accent}>{details.shortHash}</text>
								<text fg={props.theme.textMuted}>
									{" "}
									{details.author || "Unknown author"}
								</text>
								{details.date.length > 0 ? (
									<text fg={props.theme.textMuted}> {details.date}</text>
								) : null}
							</box>
						</box>

						<box
							flexGrow={1}
							minHeight={6}
							border
							borderStyle="rounded"
							borderColor={props.theme.border}
							paddingX={1}
							paddingY={0}
						>
							<scrollbox ref={props.scrollRef} flexGrow={1} height="100%">
								<text fg={props.theme.text} wrapMode="word">
									{description}
								</text>
							</scrollbox>
						</box>
					</>
				) : (
					<box marginBottom={1}>
						<text fg={props.theme.textMuted}>No blame details available.</text>
					</box>
				)}

				<box marginTop={1}>
					<text fg={props.theme.textMuted}>
						{renderHintText(props.loading, props.details)}
					</text>
				</box>
			</box>
		</box>
	);
});
