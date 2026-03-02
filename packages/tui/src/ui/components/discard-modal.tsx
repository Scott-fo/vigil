import { RGBA } from "@opentui/core";
import { memo } from "react";
import type { ResolvedTheme } from "#theme/theme";
import type { FileEntry } from "#tui/types";

export interface DiscardModalProps {
	readonly theme: ResolvedTheme;
	readonly modalBackdropColor: RGBA;
	readonly file: FileEntry;
	readonly onCancel: () => void;
	readonly onConfirm: () => void;
}

export const DiscardModal = memo(function DiscardModal(props: DiscardModalProps) {
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
			zIndex={120}
		>
			<box
				width={78}
				border
				borderStyle="rounded"
				borderColor={props.theme.borderActive}
				backgroundColor={props.theme.backgroundPanel}
				padding={1}
				flexDirection="column"
			>
				<text fg={props.theme.error}>
					<strong>Discard File Changes?</strong>
				</text>
				<box marginTop={1}>
					<text fg={props.theme.text}>
						This will remove all local changes in:
					</text>
				</box>
				<box marginTop={1} marginBottom={1}>
					<text fg={props.theme.warning}>{props.file.label}</text>
				</box>
				<text fg={props.theme.textMuted}>
					Enter confirms discard. Esc cancels.
				</text>
				<box marginTop={1} flexDirection="row" gap={1}>
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
						borderColor={props.theme.error}
						paddingX={1}
						onMouseDown={(event) => {
							event.preventDefault();
							props.onConfirm();
						}}
					>
						<text fg={props.theme.error}>Discard</text>
					</box>
				</box>
			</box>
		</box>
	);
});
