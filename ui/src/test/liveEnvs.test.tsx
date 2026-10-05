import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { consoleProblems } from "./console";
import { settleLiveEnvs, trackEnv } from "./liveEnvs";

function Late({ ms }: { ms: number }) {
  const query = useQuery({
    queryKey: ["late", ms],
    queryFn: () =>
      new Promise<string>((resolve) => {
        setTimeout(() => {
          resolve("answered");
        }, ms);
      }),
  });
  return <p>{query.data ?? "waiting"}</p>;
}

function renderLate(ms: number): QueryClient {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  trackEnv({ queryClient });
  render(
    <QueryClientProvider client={queryClient}>
      <Late ms={ms} />
    </QueryClientProvider>,
  );
  return queryClient;
}

describe("settleLiveEnvs (setup.ts, before the unmount)", () => {
  it("waits for a late answer and commits it inside act", async () => {
    const queryClient = renderLate(50);
    expect(screen.getByText("waiting")).toBeInTheDocument();
    await settleLiveEnvs();
    expect(queryClient.isFetching()).toBe(0);
    expect(screen.getByText("answered")).toBeInTheDocument();
    // No "not wrapped in act(...)" warning from the late update.
    expect(consoleProblems(false)).toEqual([]);
  });

  it("gives up without failing when an env stays busy", async () => {
    const queryClient = renderLate(60_000);
    const started = performance.now();
    await settleLiveEnvs(100);
    expect(performance.now() - started).toBeLessThan(5_000);
    expect(queryClient.isFetching()).toBe(1);
    queryClient.cancelQueries().catch(() => undefined);
  });

  it("waits for nothing under fake timers", async () => {
    vi.useFakeTimers();
    try {
      renderLate(50);
      await settleLiveEnvs();
      expect(screen.getByText("waiting")).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it("forgets the envs it settled", async () => {
    renderLate(10);
    await settleLiveEnvs();
    const started = performance.now();
    // Nothing tracked any more: returns at once.
    await settleLiveEnvs(10_000);
    expect(performance.now() - started).toBeLessThan(1_000);
  });
});
