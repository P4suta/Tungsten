import type { Locator } from "@playwright/test";

export async function typingFrame(editor: Locator, press: () => Promise<void>): Promise<number> {
  const probe = await editor.evaluateHandle((element: HTMLElement) => {
    const sample = Promise.withResolvers<number>();
    const listener = (event: KeyboardEvent): void => {
      if (!event.isTrusted) {
        sample.reject(new Error("Input measurement requires a trusted key event."));
        return;
      }
      const start = event.timeStamp;
      requestAnimationFrame(() =>
        requestAnimationFrame(() => {
          const duration = performance.now() - start;
          if (!Number.isFinite(duration) || duration < 0) {
            sample.reject(new Error("The browser input timestamp is invalid."));
          } else sample.resolve(duration);
        }),
      );
    };
    element.addEventListener("keydown", listener, { capture: true, once: true });
    return {
      sample: sample.promise,
      stop: () => element.removeEventListener("keydown", listener, true),
    };
  });
  try {
    await press();
    return await probe.evaluate(({ sample }) => sample);
  } finally {
    await probe.evaluate(({ stop }) => stop());
    await probe.dispose();
  }
}
