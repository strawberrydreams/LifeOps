export type Route =
  | { name: "home" }
  | { name: "browse"; type: string; params: Record<string, string> }
  | { name: "entity"; id: string }
  | { name: "new"; type: string }
  | { name: "page"; pageName: string };

export function parseRoute(url: string): Route {
  const [path, query = ""] = url.split("?");
  const parts = path.split("/").filter(Boolean).map(decodeURIComponent);
  const params: Record<string, string> = {};
  new URLSearchParams(query).forEach((v, k) => (params[k] = v));
  if (parts.length === 0) return { name: "home" };
  if (parts[0] === "browse" && parts[1]) return { name: "browse", type: parts[1], params };
  if (parts[0] === "entity" && parts[1]) return { name: "entity", id: parts[1] };
  if (parts[0] === "new" && parts[1]) return { name: "new", type: parts[1] };
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
