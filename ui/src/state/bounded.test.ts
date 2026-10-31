import { describe, expect, it } from "vitest";
import { pushBounded } from "./bounded";

describe("pushBounded", () => {
  it("replaces an entry with the same id and keeps only the newest", () => {
    let list: { id: string; n: number }[] = [];
    for (let n = 0; n < 1000; n++) list = pushBounded(list, { id: `t${String(n % 7)}`, n }, 5);
    expect(list).toHaveLength(5);
    expect(list.map((t) => t.n)).toEqual([995, 996, 997, 998, 999]);
    expect(pushBounded(list, { id: "t5", n: -1 }, 5).at(-1)).toEqual({ id: "t5", n: -1 });
    expect(pushBounded(list, { id: "t5", n: -1 }, 5)).toHaveLength(5);
  });
});
