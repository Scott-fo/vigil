import { describe, expect, test } from "bun:test";
import {
	calculateSidebarVirtualWindow,
	getScrollTopForVisibleRow,
} from "#ui/sidebar-virtualization";

describe("calculateSidebarVirtualWindow", () => {
	test("returns empty window when there are no rows", () => {
		expect(
			calculateSidebarVirtualWindow({
				totalRows: 0,
				scrollTop: 10,
				viewportHeight: 6,
				overscan: 4,
			}),
		).toEqual({
			start: 0,
			end: 0,
			topPadding: 0,
			bottomPadding: 0,
		});
	});

	test("calculates start and end with overscan", () => {
		expect(
			calculateSidebarVirtualWindow({
				totalRows: 100,
				scrollTop: 20,
				viewportHeight: 10,
				overscan: 3,
			}),
		).toEqual({
			start: 17,
			end: 33,
			topPadding: 17,
			bottomPadding: 67,
		});
	});

	test("clamps scrollTop near the end of the list", () => {
		expect(
			calculateSidebarVirtualWindow({
				totalRows: 12,
				scrollTop: 50,
				viewportHeight: 5,
				overscan: 2,
			}),
		).toEqual({
			start: 5,
			end: 12,
			topPadding: 5,
			bottomPadding: 0,
		});
	});
});

describe("getScrollTopForVisibleRow", () => {
	test("keeps existing scrollTop when row is already visible", () => {
		expect(getScrollTopForVisibleRow(8, 5, 6)).toBe(5);
	});

	test("scrolls up when row is above viewport", () => {
		expect(getScrollTopForVisibleRow(3, 10, 6)).toBe(3);
	});

	test("scrolls down when row is below viewport", () => {
		expect(getScrollTopForVisibleRow(15, 5, 6)).toBe(10);
	});
});
