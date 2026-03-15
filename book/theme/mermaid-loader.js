// Mermaid diagram support for mdBook.
// Loads mermaid.js lazily when a ```mermaid code block is detected.
// Automatically switches between light and dark themes.

(function () {
    "use strict";

    // Check if any mermaid blocks exist on the page.
    var blocks = document.querySelectorAll("code.language-mermaid");
    if (blocks.length === 0) return;

    // Convert code blocks to mermaid containers before loading the library.
    blocks.forEach(function (block) {
        var pre = block.parentElement;
        var div = document.createElement("div");
        div.className = "mermaid";
        div.textContent = block.textContent;
        pre.parentElement.replaceChild(div, pre);
    });

    // Detect mdBook theme to pick a matching mermaid theme.
    function getMermaidTheme() {
        var body = document.documentElement.className || "";
        if (body.indexOf("ayu") !== -1 || body.indexOf("coal") !== -1 || body.indexOf("navy") !== -1) {
            return "dark";
        }
        return "default";
    }

    // Load mermaid.js from CDN.
    var script = document.createElement("script");
    script.src = "https://cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.min.js";
    script.onload = function () {
        mermaid.initialize({
            startOnLoad: true,
            theme: getMermaidTheme(),
            flowchart: { useMaxWidth: true, htmlLabels: true },
            sequence: { useMaxWidth: true },
        });
    };
    document.head.appendChild(script);
})();
