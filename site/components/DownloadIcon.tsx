type IconName = "iphone" | "android" | "desktop" | "web" | "testflight" | "ipa" | "mac" | "windows" | "linux";

export function DownloadIcon({ name }: { name: IconName }) {
  return (
    <span className="download-icon" aria-hidden="true">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" focusable="false">
        {name === "iphone" && <><rect x="6.5" y="2.5" width="11" height="19" rx="2.5" /><path d="M10 5h4M11 18.5h2" /></>}
        {name === "android" && <><path d="M5 15a7 7 0 0 1 14 0v2H5v-2ZM7.5 5.5l1.5 2M16.5 5.5l-1.5 2M8 17v3M16 17v3" /><path d="M9 12h.01M15 12h.01" strokeWidth="2.5" /></>}
        {name === "desktop" && <><rect x="3" y="4" width="18" height="13" rx="2" /><path d="M9 21h6M12 17v4" /></>}
        {name === "web" && <><circle cx="12" cy="12" r="9" /><path d="M3 12h18M12 3c2.5 2.5 3.5 5.5 3.5 9s-1 6.5-3.5 9c-2.5-2.5-3.5-5.5-3.5-9s1-6.5 3.5-9Z" /></>}
        {name === "testflight" && <><path d="M3 12.5 20 4l-6 16-2.5-6-5 2 1.5-3.5L3 12.5Z" /><path d="m11.5 14 5-6" /></>}
        {name === "ipa" && <><path d="M6 3h8l4 4v14H6zM14 3v5h4M12 11v6m-3-3 3 3 3-3" /></>}
        {name === "mac" && <><rect x="3" y="4" width="18" height="13" rx="2" /><path d="M3 14h18M9 21h6M12 17v4" /></>}
        {name === "windows" && <><path d="M3 5.5 10.5 4v7H3zM13.5 3.5 21 2v9h-7.5zM3 13h7.5v7L3 18.5zM13.5 13H21v9l-7.5-1.5z" /></>}
        {name === "linux" && <><path d="M8 9V7a4 4 0 0 1 8 0v2c0 2 1 3 2 5 1.5 3 .5 5-1 5H7c-1.5 0-2.5-2-1-5 1-2 2-3 2-5Z" /><path d="M8 19 5 21h4l1-2m6 0 3 2h-4l-1-2M10 11l2 1 2-1M10 8h.01M14 8h.01" /></>}
      </svg>
    </span>
  );
}
