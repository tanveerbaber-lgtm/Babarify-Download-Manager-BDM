import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, EVENTS } from "../lib/api";
import type { Category, Queue } from "../lib/types";

/** Queues and categories, kept fresh when a schedule starts or stops a queue. */
export function useQueues() {
  const [queues, setQueues] = useState<Queue[]>([]);
  const [categories, setCategories] = useState<Category[]>([]);

  const refresh = useCallback(async () => {
    try {
      const [q, c] = await Promise.all([api.listQueues(), api.listCategories()]);
      setQueues(q);
      setCategories(c);
    } catch {
      // The list views still work without them.
    }
  }, []);

  useEffect(() => {
    void refresh();
    const stop = listen(EVENTS.queuesChanged, () => void refresh());
    return () => void stop.then((s) => s());
  }, [refresh]);

  return { queues, setQueues, categories, refresh };
}
