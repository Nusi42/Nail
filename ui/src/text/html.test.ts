import { escapeHtml } from "./html";

describe("escapeHtml", () => {
  it("escapes the markup characters, ampersands first", () => {
    expect(escapeHtml('a < b && c > "d"')).toBe("a &lt; b &amp;&amp; c &gt; &quot;d&quot;");
    expect(escapeHtml("&lt;")).toBe("&amp;lt;");
    expect(escapeHtml("plain")).toBe("plain");
  });
});
