import type { JSX } from "solid-js";
import { splitProps } from "solid-js";

import { cn } from "../../lib/utils";

export function Table(props: JSX.HTMLAttributes<HTMLTableElement>) {
  const [local, rest] = splitProps(props, ["class"]);
  return (
    <div class="relative w-full overflow-auto rounded-md border">
      <table class={cn("w-full caption-bottom text-sm", local.class)} {...rest} />
    </div>
  );
}

export function TableHeader(props: JSX.HTMLAttributes<HTMLTableSectionElement>) {
  const [local, rest] = splitProps(props, ["class"]);
  return <thead class={cn("[&_tr]:border-b", local.class)} {...rest} />;
}

export function TableBody(props: JSX.HTMLAttributes<HTMLTableSectionElement>) {
  const [local, rest] = splitProps(props, ["class"]);
  return <tbody class={cn("[&_tr:last-child]:border-0", local.class)} {...rest} />;
}

export function TableRow(props: JSX.HTMLAttributes<HTMLTableRowElement>) {
  const [local, rest] = splitProps(props, ["class"]);
  return <tr class={cn("border-b transition-colors hover:bg-muted/50", local.class)} {...rest} />;
}

export function TableHead(props: JSX.ThHTMLAttributes<HTMLTableCellElement>) {
  const [local, rest] = splitProps(props, ["class"]);
  return <th class={cn("h-10 px-3 text-left align-middle font-medium text-muted-foreground", local.class)} {...rest} />;
}

export function TableCell(props: JSX.TdHTMLAttributes<HTMLTableCellElement>) {
  const [local, rest] = splitProps(props, ["class"]);
  return <td class={cn("p-3 align-middle", local.class)} {...rest} />;
}
