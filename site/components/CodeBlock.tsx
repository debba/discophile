"use client";

import { useState } from "react";

/** A code sample with a copy button. Long lines scroll, so the block itself takes keyboard focus. */
export function CodeBlock({
  code,
  title,
  labels,
}: {
  code: string;
  title?: string;
  labels: { code: string; copy: string; copied: string };
}) {
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(code);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // The code stays visible and selectable.
    }
  };
  return (
    <div className="code-block">
      <div className="code-bar">
        <span>{title ?? labels.code}</span>
        <button type="button" className="copy-button" onClick={copy}>
          {copied ? labels.copied : labels.copy}
          {title && <span className="sr-only"> {title}</span>}
        </button>
      </div>
      <pre className="code" tabIndex={0} aria-label={title ?? labels.code}><code>{code}</code></pre>
      <span className="sr-only" role="status">{copied ? labels.copied : ""}</span>
    </div>
  );
}
