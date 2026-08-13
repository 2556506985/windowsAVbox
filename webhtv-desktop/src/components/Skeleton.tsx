import { useEffect, useState } from "react";

const SKELETON_COLS = 5;
const SKELETON_GAP = 12;
const COPY_HEIGHT = 74;

interface SkeletonProps {
  rows?: number;
  className?: string;
  containerWidth?: number;
}

export function SkeletonGrid({ rows = 3, className = "", containerWidth }: SkeletonProps) {
  const [size, setSize] = useState(() => computeSize(containerWidth));
  useEffect(() => {
    setSize(computeSize(containerWidth));
  }, [containerWidth]);
  useEffect(() => {
    if (containerWidth) return;
    const onResize = () => setSize(computeSize(undefined));
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, [containerWidth]);
  const itemWidth = size.width;
  return (
    <div
      className={`skeleton-grid ${className}`}
      style={{
        display: "grid",
        gridTemplateColumns: `repeat(${SKELETON_COLS}, ${itemWidth}px)`,
        gap: SKELETON_GAP,
      }}
      role="status"
      aria-label="加载中"
    >
      {Array.from({ length: rows * SKELETON_COLS }, (_, i) => (
        <SkeletonCard key={i} width={size.width} height={size.height} />
      ))}
    </div>
  );
}

function computeSize(containerWidth?: number) {
  const width = Math.max(
    120,
    ((containerWidth || window.innerWidth) - SKELETON_GAP * (SKELETON_COLS - 1)) / SKELETON_COLS,
  );
  return { width, height: Math.round(width * 1.5) + COPY_HEIGHT };
}

function SkeletonCard({ width, height }: { width: number; height: number }) {
  return (
    <div
      className="skeleton-card"
      style={{
        width,
        minHeight: height,
        background: "#171a16",
        border: "1px solid #292e28",
        borderRadius: 8,
        overflow: "hidden",
        display: "flex",
        flexDirection: "column",
      }}
    >
      <div
        className="skeleton-image"
        style={{
          width: "100%",
          aspectRatio: "2 / 3",
          background: "linear-gradient(90deg, #1e221c 25%, #252a24 50%, #1e221c 75%)",
          backgroundSize: "200% 100%",
          animation: "shimmer 1.5s infinite",
        }}
      />
      <div className="skeleton-content" style={{ padding: 12, display: "flex", flexDirection: "column", gap: 8, flex: 1 }}>
        <div className="skeleton-title" style={{ height: 16, width: "80%", background: "linear-gradient(90deg, #1e221c 25%, #252a24 50%, #1e221c 75%)", backgroundSize: "200% 100%", animation: "shimmer 1.5s infinite", borderRadius: 4 }} />
        <div className="skeleton-subtitle" style={{ height: 12, width: "60%", background: "linear-gradient(90deg, #1e221c 25%, #252a24 50%, #1e221c 75%)", backgroundSize: "200% 100%", animation: "shimmer 1.5s infinite", borderRadius: 4 }} />
        <div className="skeleton-meta" style={{ height: 12, width: "40%", background: "linear-gradient(90deg, #1e221c 25%, #252a24 50%, #1e221c 75%)", backgroundSize: "200% 100%", animation: "shimmer 1.5s infinite", borderRadius: 4 }} />
      </div>
    </div>
  );
}

export function SkeletonList({ items = 6 }: { items?: number }) {
  return (
    <div className="skeleton-list" style={{ display: "flex", flexDirection: "column", gap: 12, padding: "16px" }}>
      {Array.from({ length: items }, (_, i) => (
        <div key={i} className="skeleton-row" style={{ display: "flex", gap: 12, alignItems: "center", padding: "8px 0" }}>
          <div className="skeleton-avatar" style={{ width: 48, height: 48, borderRadius: 8, background: "linear-gradient(90deg, #1e221c 25%, #252a24 50%, #1e221c 75%)", backgroundSize: "200% 100%", animation: "shimmer 1.5s infinite" }} />
          <div className="skeleton-text" style={{ flex: 1 }}>
            <div className="skeleton-title" style={{ height: 16, width: "70%", background: "linear-gradient(90deg, #1e221c 25%, #252a24 50%, #1e221c 75%)", backgroundSize: "200% 100%", animation: "shimmer 1.5s infinite", borderRadius: 4 }} />
            <div className="skeleton-subtitle" style={{ height: 12, width: "50%", marginTop: 4, background: "linear-gradient(90deg, #1e221c 25%, #252a24 50%, #1e221c 75%)", backgroundSize: "200% 100%", animation: "shimmer 1.5s infinite", borderRadius: 4 }} />
          </div>
        </div>
      ))}
    </div>
  );
}

export function SkeletonDetail() {
  return (
    <div className="skeleton-detail" style={{ display: "grid", gridTemplateColumns: "160px 1fr", gap: 24, padding: 24 }}>
      <div className="skeleton-poster" style={{ aspectRatio: "2 / 3", background: "linear-gradient(90deg, #1e221c 25%, #252a24 50%, #1e221c 75%)", backgroundSize: "200% 100%", animation: "shimmer 1.5s infinite", borderRadius: 8 }} />
      <div className="skeleton-meta" style={{ display: "flex", flexDirection: "column", gap: 16 }}>
        <div className="skeleton-title" style={{ height: 28, width: "50%", background: "linear-gradient(90deg, #1e221c 25%, #252a24 50%, #1e221c 75%)", backgroundSize: "200% 100%", animation: "shimmer 1.5s infinite", borderRadius: 4 }} />
        <div className="skeleton-grid-small" style={{ display: "grid", gridTemplateColumns: "repeat(2, 1fr)", gap: 12 }}>
          {[1, 2, 3, 4].map((i) => (
            <div key={i} className="skeleton-field" style={{ height: 36, background: "linear-gradient(90deg, #1e221c 25%, #252a24 50%, #1e221c 75%)", backgroundSize: "200% 100%", animation: "shimmer 1.5s infinite", borderRadius: 4 }} />
          ))}
        </div>
        <div className="skeleton-content" style={{ flex: 1 }}>
          {[1, 2, 3, 4].map((i) => (
            <div key={i} className="skeleton-line" style={{ height: 14, width: `${80 + Math.random() * 20}%`, marginBottom: 8, background: "linear-gradient(90deg, #1e221c 25%, #252a24 50%, #1e221c 75%)", backgroundSize: "200% 100%", animation: "shimmer 1.5s infinite", borderRadius: 4 }} />
          ))}
        </div>
      </div>
    </div>
  );
}

export function useSkeleton<T>(data: T | null | undefined, delay = 300): boolean {
  const [showSkeleton, setShowSkeleton] = useState(false);

  useEffect(() => {
    if (data) {
      setShowSkeleton(false);
      return;
    }
    const timer = setTimeout(() => setShowSkeleton(true), delay);
    return () => clearTimeout(timer);
  }, [data, delay]);

  return showSkeleton;
}
