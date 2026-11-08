// src/useConsoleLogs.ts
import {useCallback, useEffect, useRef, useState} from "react";
import type {MessagePayload} from "./ConsolePage";

const MAX_CONSOLE_LOGS = 500;
const CONSOLE_LOG_STORAGE_KEY = 'pyappifyConsoleLogs';
const CONSOLE_LOG_PERSIST_INTERVAL_MS = 1000;

export type ConsoleLogs = Record<string, MessagePayload[]>;

const loadConsoleLogs = (): ConsoleLogs => {
    try {
        const stored = localStorage.getItem(CONSOLE_LOG_STORAGE_KEY);
        if (!stored) return {};
        const parsed = JSON.parse(stored);
        return parsed && typeof parsed === 'object' ? parsed : {};
    } catch (error) {
        console.warn('Failed to restore console logs:', error);
        return {};
    }
};

/**
 * Per-app console log history, keyed by app name, restored from and persisted
 * to localStorage so logs survive a reload.
 */
export function useConsoleLogs() {
    const [consoleLogs, setConsoleLogs] = useState<ConsoleLogs>(loadConsoleLogs);

    // Persisting serializes every app's log history, so do it at most once per
    // interval (and on unload) instead of on every streamed log line.
    const consoleLogsRef = useRef(consoleLogs);
    const consoleLogsDirtyRef = useRef(false);
    useEffect(() => {
        consoleLogsRef.current = consoleLogs;
        consoleLogsDirtyRef.current = true;
    }, [consoleLogs]);
    useEffect(() => {
        const persist = () => {
            if (!consoleLogsDirtyRef.current) return;
            consoleLogsDirtyRef.current = false;
            try {
                localStorage.setItem(CONSOLE_LOG_STORAGE_KEY, JSON.stringify(consoleLogsRef.current));
            } catch (error) {
                console.warn('Failed to persist console logs:', error);
            }
        };
        const intervalId = window.setInterval(persist, CONSOLE_LOG_PERSIST_INTERVAL_MS);
        window.addEventListener('beforeunload', persist);
        return () => {
            window.clearInterval(intervalId);
            window.removeEventListener('beforeunload', persist);
            persist();
        };
    }, []);

    /** Appends a log line; an `update` entry replaces the previous line (progress output). */
    const addConsoleLog = useCallback((logEntry: MessagePayload) => {
        setConsoleLogs(previous => {
            const appLogs = previous[logEntry.app_name] ?? [];
            let nextAppLogs: MessagePayload[];
            if (logEntry.update && appLogs.length > 0) {
                nextAppLogs = [...appLogs];
                nextAppLogs[nextAppLogs.length - 1] = logEntry;
            } else {
                nextAppLogs = [...appLogs, logEntry];
            }
            if (nextAppLogs.length > MAX_CONSOLE_LOGS) {
                nextAppLogs = nextAppLogs.slice(-MAX_CONSOLE_LOGS);
            }
            return {...previous, [logEntry.app_name]: nextAppLogs};
        });
    }, []);

    /** Clears the app's history and starts a new session with `message`. */
    const beginConsoleSession = useCallback((appName: string, message: string) => {
        setConsoleLogs(previous => ({
            ...previous,
            [appName]: [{message, app_name: appName}],
        }));
    }, []);

    /** Starts a new session unless one is still in progress (not yet finished). */
    const ensureActiveConsoleSession = useCallback((appName: string, message: string) => {
        setConsoleLogs(previous => {
            const existingLogs = previous[appName] ?? [];
            const hasActiveSession = existingLogs.length > 0 && !existingLogs.some(log => log.finished);
            return hasActiveSession
                ? previous
                : {...previous, [appName]: [{message, app_name: appName}]};
        });
    }, []);

    /** Seeds the app's history with `entry` only if it has no logs at all. */
    const seedConsoleSession = useCallback((entry: MessagePayload) => {
        setConsoleLogs(previous => previous[entry.app_name]?.length
            ? previous
            : {...previous, [entry.app_name]: [entry]});
    }, []);

    return {consoleLogs, addConsoleLog, beginConsoleSession, ensureActiveConsoleSession, seedConsoleSession};
}
