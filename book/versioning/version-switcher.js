// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
//
// The documentation version selector, and the notice on a page that is not
// the latest release's. scripts/build_docs_site.py injects this into every
// mdBook page of every version, with three data attributes on the tag:
//
//   data-a2a-version  the version the page belongs to ("0.14", or "dev")
//   data-a2a-root     the relative path from the page to the site root
//   data-a2a-page     the page's path within its version ("concepts/x.html")
//
// It reads versions.json from the site root. Without JavaScript, or when that
// file cannot be read, the page is unchanged: every version is still a plain
// static book at its own URL.
(function () {
  "use strict";
  var tag = document.currentScript;
  if (!tag) {
    return;
  }
  var version = tag.getAttribute("data-a2a-version");
  var root = tag.getAttribute("data-a2a-root") || "";
  var page = tag.getAttribute("data-a2a-page") || "";

  function hrefFor(entry, withPage) {
    return root + entry.path + (withPage ? page : "");
  }

  // The same page in another version when it exists there, else that
  // version's front page: pages are added and renamed between releases.
  function go(entry) {
    var url = hrefFor(entry, true);
    fetch(url, { method: "HEAD" })
      .then(function (r) {
        window.location.href = r.ok ? url : hrefFor(entry, false);
      })
      .catch(function () {
        window.location.href = hrefFor(entry, false);
      });
  }

  function label(entry, latest) {
    if (entry.label === "dev") {
      return "dev (main)";
    }
    return entry.label === latest ? entry.label + " (latest)" : entry.label;
  }

  function selector(index) {
    var entries = index.versions.concat([index.dev]);
    var select = document.createElement("select");
    select.className = "a2a-version-select";
    select.setAttribute("aria-label", "Documentation version");
    select.title = "Documentation version";
    entries.forEach(function (entry) {
      var option = document.createElement("option");
      option.value = entry.label;
      option.textContent = label(entry, index.latest);
      if (entry.label === version) {
        option.selected = true;
      }
      select.appendChild(option);
    });
    select.addEventListener("change", function () {
      go(entries[select.selectedIndex]);
    });
    return select;
  }

  function notice(index) {
    var latest = index.versions[0];
    var box = document.createElement("div");
    box.className = "a2a-version-notice";
    box.setAttribute("role", "note");
    var text =
      version === "dev"
        ? "This is the development documentation, built from the main branch: " +
          "it can describe changes no release has yet. The latest release is " +
          latest.release + ". "
        : "This is the documentation for a2a-rust " + version + ". The latest " +
          "release is " + latest.release + ". ";
    box.appendChild(document.createTextNode(text));
    var link = document.createElement("a");
    link.href = hrefFor(latest, true);
    link.textContent = "Read this page for " + latest.release + ".";
    link.addEventListener("click", function (e) {
      e.preventDefault();
      go(latest);
    });
    box.appendChild(link);
    return box;
  }

  function render(index) {
    var bar = document.querySelector("#menu-bar .right-buttons");
    if (bar) {
      bar.insertBefore(selector(index), bar.firstChild);
    }
    var latestEntry = index.versions[0];
    var main = document.querySelector("#content main");
    if (main && latestEntry && version !== latestEntry.label) {
      main.insertBefore(notice(index), main.firstChild);
    }
  }

  fetch(root + "versions.json")
    .then(function (r) {
      return r.ok ? r.json() : null;
    })
    .then(function (index) {
      if (index && index.versions && index.versions.length && index.dev) {
        render(index);
      }
    })
    .catch(function () {
      // No index: leave the page as it is.
    });
})();
