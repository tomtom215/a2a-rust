// Mermaid diagram support for mdBook.
// Loads mermaid.js lazily when a ```mermaid code block is detected.

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

    // Load mermaid.js from CDN.
    var script = document.createElement("script");
    script.src = "https://cdn.jsdelivr.net/npm/mermaid@10/dist/mermaid.min.js";
    script.onload = function () {
        mermaid.initialize({ startOnLoad: true, theme: "default" });
    };
    document.head.appendChild(script);
})();
