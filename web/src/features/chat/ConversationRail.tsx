import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type PointerEvent,
  type RefObject,
  type WheelEvent,
} from "react";

export type ConversationAnchor = {
  id: string;
  label: string;
  role: "user" | "assistant";
};

type ConversationRailProps = {
  anchors: ConversationAnchor[];
  containerRef: RefObject<HTMLDivElement | null>;
  contentRef: RefObject<HTMLDivElement | null>;
  onManualNavigate: () => void;
};

type PositionedAnchor = ConversationAnchor & {
  ratio: number;
  scrollTop: number;
};

type RailLayout = {
  markers: PositionedAnchor[];
  overflow: boolean;
  progress: number;
};

const TRACK_INSET_PX = 7;

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

function maxScrollTop(container: HTMLDivElement): number {
  return Math.max(0, container.scrollHeight - container.clientHeight);
}

function currentProgress(container: HTMLDivElement): number {
  const max = maxScrollTop(container);
  return max === 0 ? 1 : clamp(container.scrollTop / max, 0, 1);
}

function calculateLayout(
  container: HTMLDivElement,
  anchors: ConversationAnchor[],
): RailLayout {
  const max = maxScrollTop(container);
  const containerTop = container.getBoundingClientRect().top;
  const elements = Array.from(
    container.querySelectorAll<HTMLElement>("[data-history-anchor]"),
  );
  const elementById = new Map(
    elements.map((element) => [element.dataset.historyAnchor, element]),
  );
  const markers = anchors.flatMap((anchor) => {
    const element = elementById.get(anchor.id);
    if (!element || max === 0) {
      return [];
    }
    const contentTop =
      element.getBoundingClientRect().top - containerTop + container.scrollTop;
    const scrollTop = clamp(contentTop - 12, 0, max);
    return [{ ...anchor, ratio: scrollTop / max, scrollTop }];
  });

  return {
    markers,
    overflow: max > 0,
    progress: currentProgress(container),
  };
}

export function ConversationRail({
  anchors,
  containerRef,
  contentRef,
  onManualNavigate,
}: ConversationRailProps) {
  const railRef = useRef<HTMLDivElement>(null);
  const frameRef = useRef<number | null>(null);
  const draggingRef = useRef(false);
  const [layout, setLayout] = useState<RailLayout>({
    markers: [],
    overflow: false,
    progress: 1,
  });

  const syncLayout = useCallback(() => {
    const container = containerRef.current;
    if (!container) {
      return;
    }
    setLayout(calculateLayout(container, anchors));
  }, [anchors, containerRef]);

  const scheduleLayout = useCallback(() => {
    if (frameRef.current != null) {
      cancelAnimationFrame(frameRef.current);
    }
    frameRef.current = requestAnimationFrame(() => {
      frameRef.current = null;
      syncLayout();
    });
  }, [syncLayout]);

  const syncProgress = useCallback(() => {
    const container = containerRef.current;
    if (!container) {
      return;
    }
    const progress = currentProgress(container);
    setLayout((current) =>
      current.progress === progress ? current : { ...current, progress },
    );
  }, [containerRef]);

  useEffect(() => {
    const container = containerRef.current;
    const content = contentRef.current;
    if (!container || !content) {
      return;
    }

    const handleScroll = () => syncProgress();
    const handleResize = () => scheduleLayout();
    container.addEventListener("scroll", handleScroll, { passive: true });
    window.addEventListener("resize", handleResize);

    const observer =
      typeof ResizeObserver === "undefined"
        ? null
        : new ResizeObserver(scheduleLayout);
    observer?.observe(container);
    observer?.observe(content);
    scheduleLayout();

    return () => {
      container.removeEventListener("scroll", handleScroll);
      window.removeEventListener("resize", handleResize);
      observer?.disconnect();
      if (frameRef.current != null) {
        cancelAnimationFrame(frameRef.current);
        frameRef.current = null;
      }
    };
  }, [containerRef, contentRef, scheduleLayout, syncProgress]);

  function seekTo(scrollTop: number, behavior: ScrollBehavior) {
    const container = containerRef.current;
    if (!container) {
      return;
    }
    onManualNavigate();
    if (typeof container.scrollTo === "function") {
      container.scrollTo({ top: scrollTop, behavior });
    } else {
      container.scrollTop = scrollTop;
    }
    syncProgress();
  }

  function seekToPointer(clientY: number) {
    const rail = railRef.current;
    const container = containerRef.current;
    if (!rail || !container) {
      return;
    }
    const rect = rail.getBoundingClientRect();
    const trackHeight = Math.max(1, rect.height - TRACK_INSET_PX * 2);
    const ratio = clamp(
      (clientY - rect.top - TRACK_INSET_PX) / trackHeight,
      0,
      1,
    );
    seekTo(maxScrollTop(container) * ratio, "auto");
  }

  function handlePointerDown(event: PointerEvent<HTMLDivElement>) {
    draggingRef.current = true;
    event.currentTarget.setPointerCapture?.(event.pointerId);
    seekToPointer(event.clientY);
  }

  function handlePointerMove(event: PointerEvent<HTMLDivElement>) {
    if (draggingRef.current) {
      seekToPointer(event.clientY);
    }
  }

  function handlePointerEnd(event: PointerEvent<HTMLDivElement>) {
    draggingRef.current = false;
    if (event.currentTarget.hasPointerCapture?.(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  }

  function handleWheel(event: WheelEvent<HTMLDivElement>) {
    const container = containerRef.current;
    if (!container) {
      return;
    }
    event.preventDefault();
    seekTo(container.scrollTop + event.deltaY, "auto");
  }

  function handleKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    const container = containerRef.current;
    if (!container) {
      return;
    }
    const page = container.clientHeight * 0.8;
    const targets: Partial<Record<string, number>> = {
      ArrowUp: container.scrollTop - 48,
      ArrowDown: container.scrollTop + 48,
      PageUp: container.scrollTop - page,
      PageDown: container.scrollTop + page,
      Home: 0,
      End: maxScrollTop(container),
    };
    const target = targets[event.key];
    if (target == null) {
      return;
    }
    event.preventDefault();
    seekTo(target, "auto");
  }

  if (!layout.overflow) {
    return null;
  }

  const activeMarker = layout.markers.reduce<PositionedAnchor | null>(
    (nearest, marker) =>
      nearest == null ||
      Math.abs(marker.ratio - layout.progress) <
        Math.abs(nearest.ratio - layout.progress)
        ? marker
        : nearest,
    null,
  );

  return (
    <div
      aria-controls="message-timeline-scrollport"
      aria-label="Conversation position"
      aria-orientation="vertical"
      aria-valuemax={100}
      aria-valuemin={0}
      aria-valuenow={Math.round(layout.progress * 100)}
      className="conversation-rail"
      onKeyDown={handleKeyDown}
      onPointerCancel={handlePointerEnd}
      onPointerDown={handlePointerDown}
      onPointerMove={handlePointerMove}
      onPointerUp={handlePointerEnd}
      onWheel={handleWheel}
      ref={railRef}
      role="scrollbar"
      tabIndex={0}
    >
      <div className="conversation-rail-scale">
        <span aria-hidden="true" className="conversation-rail-track" />
        {layout.markers.map((marker) => (
          <button
            aria-current={
              activeMarker?.id === marker.id ? "location" : undefined
            }
            aria-label={`Jump to ${marker.role} message: ${marker.label}`}
            className={`conversation-marker conversation-marker-${marker.role}`}
            key={marker.id}
            onClick={(event) => {
              event.stopPropagation();
              seekTo(marker.scrollTop, "smooth");
            }}
            onPointerDown={(event) => event.stopPropagation()}
            style={{ top: `${marker.ratio * 100}%` }}
            title={`${marker.role}: ${marker.label}`}
            type="button"
          />
        ))}
        <span
          aria-hidden="true"
          className="conversation-rail-thumb"
          style={{ top: `${layout.progress * 100}%` }}
        />
      </div>
    </div>
  );
}
