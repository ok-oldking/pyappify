// src/utils.ts
import {invoke} from "@tauri-apps/api/core";

export async function invokeTauriCommandWrapper<T>(
    command: string,
    args: Record<string, unknown> | undefined,
    onSuccess: (result: T) => Promise<void> | void,
    onError: (errorMessage: string, rawError: unknown) => void
) {
    try {
        const result = await invoke<T>(command, args);
        const successResult = onSuccess(result);
        if (successResult instanceof Promise) {
            await successResult;
        }
    } catch (err) {
        const errorMessage = (typeof err === 'object' && err !== null && 'message' in err) ? String((err as {
            message: unknown
        }).message) : String(err);
        onError(errorMessage, err);
    }
}
