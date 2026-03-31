import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./BarloBar.css";

interface StatusItemInfo {
  icon_base64: string;
  click_x: number;
  click_y: number;
}

export function BarloBar() {
  const [items, setItems] = useState<StatusItemInfo[]>([]);
  const iconsRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (items.length === 0 || !iconsRef.current) {
      invoke("resize_barlo_bar", { contentWidth: 0 });
      return;
    }

    const observer = new ResizeObserver((entries) => {
      const w = entries[0]?.contentRect.width ?? 0;
      if (w > 0) {
        invoke("resize_barlo_bar", { contentWidth: w });
      }
    });

    observer.observe(iconsRef.current);
    return () => observer.disconnect();
  }, [items]);

  useEffect(() => {
    const unlisten = listen<{ hidden: boolean }>("barlo-icons-state", async (event) => {
      if (event.payload.hidden) {
        try {
          const result = await invoke<StatusItemInfo[]>("get_hidden_status_items");
          setItems(result);
        } catch {
          setItems([]);
        }
      } else {
        setItems([]);
      }
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  if (items.length === 0) {
    return <div className="bar empty" />;
  }

  return (
    <div className="bar">
      <div ref={iconsRef} className="bar-icons">
        {items.map((item, i) => (
          <button
            key={i}
            className="bar-item"
            onClick={() => invoke("activate_status_item", { clickX: item.click_x, clickY: item.click_y })}
          >
            <img
              src={`data:image/png;base64,${item.icon_base64}`}
              className="item-icon"
              draggable={false}
              alt=""
            />
          </button>
        ))}
      </div>
    </div>
  );
}
