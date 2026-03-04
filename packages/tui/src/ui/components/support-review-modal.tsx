import type { RGBA } from "@opentui/core";
import { memo } from "react";
import type { ResolvedTheme } from "#theme/theme.ts";

export interface SupportReviewModalProps {
	readonly theme: ResolvedTheme;
	readonly modalBackdropColor: RGBA;
	readonly onCancel: () => void;
	readonly onConfirm: () => void;
}

export const SupportReviewModal = memo(function SupportReviewModal(
	props: SupportReviewModalProps,
) {
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
			zIndex={115}
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
					<strong>Generate Review</strong>
				</text>
				<box marginTop={1}>
					<text fg={props.theme.text}>Generate review for this diff?</text>
				</box>
				<box marginTop={1} marginBottom={1}>
					<text fg={props.theme.textMuted}>
						Enter sends request. Esc cancels.
					</text>
				</box>
				<box flexDirection="row" gap={1}>
					<box
						border
						borderStyle="rounded"
						borderColor={props.theme.border}
						paddingX={1}
						onMouseDown={(event) => {
							event.preventDefault();
							props.onCancel();
						}}
					>
						<text fg={props.theme.text}>Cancel</text>
					</box>
					<box
						border
						borderStyle="rounded"
						borderColor={props.theme.primary}
						paddingX={1}
						onMouseDown={(event) => {
							event.preventDefault();
							props.onConfirm();
						}}
					>
						<text fg={props.theme.primary}>Generate</text>
					</box>
				</box>
			</box>
		</box>
	);
});
