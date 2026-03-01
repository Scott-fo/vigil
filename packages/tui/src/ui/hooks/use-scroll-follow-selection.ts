import type { ScrollBoxRenderable } from "@opentui/core";
import { type RefObject, useEffect } from "react";

function keepSelectionVisible(
	scroll: ScrollBoxRenderable,
	selectedRowId: string,
): void {
	const children = scroll.getChildren();
	const target = children.find((child) => child.id === selectedRowId);
	if (!target) {
		return;
	}

	const y = target.y - scroll.y;
	if (y >= scroll.height) {
		scroll.scrollBy(y - scroll.height + 1);
		return;
	}
	if (y < 0) {
		scroll.scrollBy(y);
		if (children[0]?.id === target.id) {
			scroll.scrollTo(0);
		}
	}
}

interface UseScrollFollowSelectionOptions {
	readonly scrollRef: RefObject<ScrollBoxRenderable | null>;
	readonly selectedRowId: string | null;
}

export function useScrollFollowSelection(
	options: UseScrollFollowSelectionOptions,
): void {
	useEffect(() => {
		const selectedRowId = options.selectedRowId;
		if (!selectedRowId) {
			return;
		}
		const scroll = options.scrollRef.current;
		if (!scroll) {
			return;
		}

		// Run immediately and on next tick so layouted row coordinates are available.
		keepSelectionVisible(scroll, selectedRowId);
		const timeout = setTimeout(() => {
			keepSelectionVisible(scroll, selectedRowId);
		}, 0);
		timeout.unref?.();
		return () => {
			clearTimeout(timeout);
		};
	}, [options.scrollRef, options.selectedRowId]);
}
