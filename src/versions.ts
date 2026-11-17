// src/versions.ts
import type {VersionActionType} from "./updateProgress";

type ParsedVersion = {
    parts: [number, number, number, number];
    rank: number;
    number: number;
    word: string;
};

// The tag format and ordering of parse_version_tag in src-tauri/src/git.rs, also used
// by pyappify-action and pyappify-python: [prefix] [v] N[.N[.N[.N]]] [word [number]].
// A single number needs the v (v5, app-v5). Missing parts count as 0.
// dev < alpha (a) < beta (b) < any other word < rc (c, pre, preview) < release < post.
const VERSION_RE = /^([A-Za-z][A-Za-z0-9_.-]*?[-_/])??([vV])?(\d+(?:\.\d+){0,3})(?:[-._]?([A-Za-z]+)(?:[-._]?(\d+))?)?$/;
const OTHER_RANK = 3;
const RELEASE_RANK = 5;
const WORD_RANK: Record<string, number> = {
    dev: 0, alpha: 1, a: 1, beta: 2, b: 2, rc: 4, c: 4, pre: 4, preview: 4, post: 6,
};
// Without version tags, an app's versions are its most recent commits, by short hash.
const COMMIT_RE = /^[0-9a-f]{7,40}$/i;

const parseVersion = (version: string): ParsedVersion | null => {
    const match = version.match(VERSION_RE);
    if (!match) return null;
    const numbers = match[3].split('.').map(Number);
    if (numbers.length === 1 && !match[2]) return null;
    const parts = [0, 0, 0, 0] as [number, number, number, number];
    numbers.forEach((value, index) => { parts[index] = value; });
    const word = match[4] ? match[4].toLowerCase() : '';
    const rank = word ? (WORD_RANK[word] ?? OTHER_RANK) : RELEASE_RANK;
    return {parts, rank, number: match[5] ? Number(match[5]) : -1, word: rank === OTHER_RANK ? word : ''};
};

/** A commit version: an app without version tags lists its commits instead. */
export const isCommitVersion = (version: string): boolean => COMMIT_RE.test(version) && !parseVersion(version);

/**
 * Negative when v1 < v2, positive when v1 > v2, zero when equal. Commit versions are
 * ordered by `order`, the app's available versions (newest first); a listed commit is
 * newer than one that is not listed.
 */
export const compareVersions = (v1: string, v2: string, order: readonly string[] = []): number => {
    if (isCommitVersion(v1) || isCommitVersion(v2)) {
        if (v1 === v2) return 0;
        const left = order.indexOf(v1);
        const right = order.indexOf(v2);
        if (left !== -1 && right !== -1) return right - left;
        if (left !== -1) return 1;
        if (right !== -1) return -1;
        return 0;
    }
    const left = parseVersion(v1);
    const right = parseVersion(v2);
    if (!left || !right) return v1.localeCompare(v2, undefined, {numeric: true, sensitivity: 'base'});

    for (let index = 0; index < 4; index++) {
        if (left.parts[index] !== right.parts[index]) return left.parts[index] - right.parts[index];
    }
    if (left.rank !== right.rank) return left.rank - right.rank;
    if (left.number !== right.number) return left.number - right.number;
    return left.word < right.word ? -1 : left.word > right.word ? 1 : 0;
};

/** A release (or post release) tag, or a commit version. */
export const isReleaseVersion = (version: string): boolean => {
    if (isCommitVersion(version)) return true;
    const parsed = parseVersion(version);
    return parsed !== null && parsed.rank >= RELEASE_RANK;
};

export const getVersionChannelLabelKey = (version: string): string => (
    isReleaseVersion(version) ? 'Release Version' : 'Test Version'
);

export const getVersionActionType = (
    targetVersion: string,
    currentVersion: string | null,
    sameVersionAction: VersionActionType = 'Set',
    order: readonly string[] = [],
): VersionActionType => {
    if (!targetVersion) return sameVersionAction;
    if (!currentVersion) return sameVersionAction;
    const comparison = compareVersions(targetVersion, currentVersion, order);
    return comparison > 0 ? 'Upgrade' : comparison < 0 ? 'Downgrade' : sameVersionAction;
};

export const getVersionActionProgressKey = (actionType: VersionActionType): string => {
    if (actionType === 'Upgrade') return 'Upgrading...';
    if (actionType === 'Downgrade') return 'Downgrading...';
    return 'Setting...';
};
