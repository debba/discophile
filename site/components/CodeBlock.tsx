"use client";

import { useEffect, useRef, useState, type ReactNode } from "react";

type CodeLabels = { code: string; copy: string; copied: string; copyFailed?: string };

/** A code sample with a copy button. Long lines scroll, so the block itself takes keyboard focus. */
export function CodeBlock({
  code,
  title,
  labels,
  actions,
}: {
  code: string;
  title?: string;
  labels: CodeLabels;
  actions?: ReactNode;
}) {
  return (
    <div className="code-block">
      <div className="code-bar">
        <span>{title ?? labels.code}</span>
        <div className="code-actions">
          {actions}
          <CopyCodeButton code={code} title={title} labels={labels} />
        </div>
      </div>
      <pre className="code" tabIndex={0} role="region" aria-label={title ?? labels.code}><code>{code}</code></pre>
    </div>
  );
}

export function CopyCodeButton({ code, title, labels, className = "copy-button", disabled = false }: {
  code: string; title?: string; labels: CodeLabels; className?: string; disabled?: boolean;
}) {
  const [copiedCode, setCopiedCode] = useState<string | null>(null);
  const [failedCode, setFailedCode] = useState<string | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => () => { if (timer.current) clearTimeout(timer.current); }, []);
  const copied = copiedCode === code;
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(code);
      setFailedCode(null);
      setCopiedCode(code);
      if (timer.current) clearTimeout(timer.current);
      timer.current = setTimeout(() => setCopiedCode(null), 2000);
    } catch {
      setCopiedCode(null);
      setFailedCode(code);
    }
  };
  return (
    <>
      <button type="button" className={className} onClick={copy} disabled={disabled}>
        {copied ? labels.copied : labels.copy}
        {title && <span className="sr-only"> {title}</span>}
      </button>
      <span className="sr-only" role="status">{copied ? labels.copied : ""}</span>
      {failedCode === code && labels.copyFailed && <p className="copy-error" role="alert">{labels.copyFailed}</p>}
    </>
  );
}
