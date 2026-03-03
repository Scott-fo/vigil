interface SidebarVirtualWindowOptions {
	readonly totalRows: number;
	readonly scrollTop: number;
	readonly viewportHeight: number;
	readonly overscan: number;
}

export interface SidebarVirtualWindow {
	readonly start: number;
	readonly end: number;
	readonly topPadding: number;
	readonly bottomPadding: number;
}

function clamp(value: number, min: number, max: number): number {
	return Math.max(min, Math.min(value, max));
}

export function calculateSidebarVirtualWindow(
	options: SidebarVirtualWindowOptions,
): SidebarVirtualWindow {
	if (options.totalRows <= 0) {
		return {
			start: 0,
			end: 0,
			topPadding: 0,
			bottomPadding: 0,
		};
	}

	const viewportHeight = Math.max(1, Math.floor(options.viewportHeight));
	const maxScrollTop = Math.max(options.totalRows - viewportHeight, 0);
	const scrollTop = clamp(Math.floor(options.scrollTop), 0, maxScrollTop);
	const overscan = Math.max(0, Math.floor(options.overscan));

	const visibleStart = scrollTop;
	const visibleEnd = Math.min(options.totalRows, visibleStart + viewportHeight);
	const start = Math.max(0, visibleStart - overscan);
	const end = Math.min(options.totalRows, visibleEnd + overscan);

	return {
		start,
		end,
		topPadding: start,
		bottomPadding: options.totalRows - end,
	};
}

export function getScrollTopForVisibleRow(
	rowIndex: number,
	scrollTop: number,
	viewportHeight: number,
): number {
	if (rowIndex < 0) {
		return Math.max(0, Math.floor(scrollTop));
	}

	const height = Math.max(1, Math.floor(viewportHeight));
	const top = Math.max(0, Math.floor(scrollTop));
	const bottom = top + height - 1;

	if (rowIndex < top) {
		return rowIndex;
	}

	if (rowIndex > bottom) {
		return rowIndex - height + 1;
	}

	return top;
}
