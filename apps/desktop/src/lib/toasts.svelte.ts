export interface Toast {
  id: number;
  kind: "info" | "success" | "error";
  title: string;
  body?: string;
  timeout?: number;
  action?: { label: string; run: () => void };
}

class Toasts {
  items = $state<Toast[]>([]);
  private seq = 0;

  push(t: Omit<Toast, "id">) {
    const id = ++this.seq;
    this.items = [...this.items.slice(-3), { ...t, id }];
    setTimeout(() => this.dismiss(id), t.timeout ?? 5000);
    return id;
  }

  dismiss(id: number) {
    this.items = this.items.filter((t) => t.id !== id);
  }
}

export const toasts = new Toasts();
