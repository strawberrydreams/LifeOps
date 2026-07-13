export type Route =
  | { name: "home" }
  | { name: "browse"; type: string; params: Record<string, string> }
  | { name: "entity"; id: string }
  | { name: "new"; type: string }
  | { name: "page"; pageName: string }
  | { name: "type-new" }
  | { name: "type-edit"; type: string }
  | { name: "page-new" }
  | { name: "page-edit"; pageName: string };

function safeDecodeURIComponent(value: string): string {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
}

export function parseRoute(url: string): Route {
  const [path, query = ""] = url.split("?");
  const parts = path.split("/").filter(Boolean).map(safeDecodeURIComponent);
  const params: Record<string, string> = {};
  new URLSearchParams(query).forEach((v, k) => (params[k] = v));
  if (parts.length === 0) return { name: "home" };
  if (parts[0] === "types" && parts[1] === "new") return { name: "type-new" };
  if (parts[0] === "types" && parts[1] && parts[2] === "edit") return { name: "type-edit", type: parts[1] };
  if (parts[0] === "browse" && parts[1]) return { name: "browse", type: parts[1], params };
  if (parts[0] === "entity" && parts[1]) return { name: "entity", id: parts[1] };
  if (parts[0] === "new" && parts[1]) return { name: "new", type: parts[1] };
  if (parts[0] === "pages" && parts[1] === "new") return { name: "page-new" };
  if (parts[0] === "pages" && parts[1] && parts[2] === "edit") return { name: "page-edit", pageName: parts[1] };
  if (parts[0] === "pages" && parts[1]) return { name: "page", pageName: parts[1] };
  return { name: "home" };
}

export const router = $state<{ route: Route }>({
  route: parseRoute(location.pathname + location.search),
});

export function navigate(path: string) {
  history.pushState({}, "", path);
  router.route = parseRoute(path);
}

if (typeof window !== "undefined") {
  window.addEventListener("popstate", () => {
    router.route = parseRoute(location.pathname + location.search);
  });
}
