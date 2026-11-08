// src/versions.ts
import type {VersionActionType} from "./updateProgress";

type ParsedVersion = {
    major: number;
    minor: number;
    patch: number;
    prerelease: null | {
        stage: 'alpha' | 'beta' | 'rc';
        number: number | null;
    };
};

// Keep the tag format and ordering in sync with parse_version_tag and
// Prerelease in src-tauri/src/git.rs (alpha < beta < rc < release).
const parseVersion = (version: string): ParsedVersion | null => {
    const match = version.match(/^v?(\d+)\.(\d+)\.(\d+)(?:(?:-|\.)(alpha|beta|rc)(?:\.(\d+))?)?$/);
    if (!match) return null;
    return {
        major: Number(match[1]),
        minor: Number(match[2]),
        patch: Number(match[3]),
        prerelease: match[4]
            ? {stage: match[4] as 'alpha' | 'beta' | 'rc', number: match[5] ? Number(match[5]) : null}
            : null,
    };
};

const prereleaseRank = (version: ParsedVersion): number => {
    if (!version.prerelease) return 3;
    if (version.prerelease.stage === 'rc') return 2;
    if (version.prerelease.stage === 'beta') return 1;
    return 0;
};

/** Negative when v1 < v2, positive when v1 > v2, zero when equal. */
export const compareVersions = (v1: string, v2: string): number => {
    const left = parseVersion(v1);
    const right = parseVersion(v2);
    if (!left || !right) return v1.localeCompare(v2, undefined, {numeric: true, sensitivity: 'base'});

    const numericParts: Array<keyof Pick<ParsedVersion, 'major' | 'minor' | 'patch'>> = ['major', 'minor', 'patch'];
    for (const part of numericParts) {
        if (left[part] !== right[part]) return left[part] - right[part];
    }

    const rankDiff = prereleaseRank(left) - prereleaseRank(right);
    if (rankDiff !== 0) return rankDiff;

    const leftNumber = left.prerelease?.number ?? -1;
    const rightNumber = right.prerelease?.number ?? -1;
    return leftNumber - rightNumber;
};

export const isReleaseVersion = (version: string): boolean => parseVersion(version)?.prerelease === null;

export const getVersionChannelLabelKey = (version: string): string => (
    isReleaseVersion(version) ? 'Release Version' : 'Test Version'
);

export const getVersionActionType = (
    targetVersion: string,
    currentVersion: string | null,
    sameVersionAction: VersionActionType = 'Set',
): VersionActionType => {
    if (!targetVersion) return sameVersionAction;
    if (!currentVersion) return sameVersionAction;
    const comparison = compareVersions(targetVersion, currentVersion);
    return comparison > 0 ? 'Upgrade' : comparison < 0 ? 'Downgrade' : sameVersionAction;
};

export const getVersionActionProgressKey = (actionType: VersionActionType): string => {
    if (actionType === 'Upgrade') return 'Upgrading...';
    if (actionType === 'Downgrade') return 'Downgrading...';
    return 'Setting...';
};
