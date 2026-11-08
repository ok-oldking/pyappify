// src/types.ts
import type {VersionActionType} from "./updateProgress";

export interface Profile {
    name: string;
    main_script: string;
    admin: boolean;
    requirements: string;
    python_path: string;
}

/** The managed application, as emitted by the backend's "app" event. */
export interface AppInfo {
    name: string;
    icon: string;
    website: string | null;
    path: string;
    current_version: string | null;
    available_versions: string[];
    running: boolean;
    installed: boolean;
    update_method: string;
    auto_start: boolean;
    update_state: 'idle' | 'updating' | 'failed';
    update_target_version: string | null;
    update_error: string | null;
    profiles: Profile[];
    current_profile: string;
    show_add_defender: boolean;
}

export type StatusState = {
    loading?: boolean;
    error?: string | null;
    info?: string | null;
    messageLoading?: boolean;
};

export type InlineUpdateLogState = {
    version: string;
    actionType: VersionActionType;
    isConfirming: boolean;
    completed?: boolean;
    failed?: boolean;
};

export type InlineConsoleKind = 'start' | 'update';

export type VersionChangeRequest = {
    appName: string;
    version: string;
    actionType: VersionActionType;
};

export const UPDATE_METHOD_OPTIONS = [
    'MANUAL_UPDATE',
    'AUTO_UPDATE',
    'AUTO_UPDATE_PRE_RELEASE',
] as const;

/** The app's current profile if it still exists, else its first profile. */
export const pickInitialProfile = (app: AppInfo, fallback: string): string =>
    app.profiles?.some(p => p.name === app.current_profile)
        ? app.current_profile
        : app.profiles?.[0]?.name || fallback;

/** Returns a copy of `record` without `key`. */
export const withoutKey = <T>(record: Record<string, T>, key: string): Record<string, T> => {
    const next = {...record};
    delete next[key];
    return next;
};
