export type VersionChangeProgressPhase =
    | 'preparing'
    | 'checkout'
    | 'files'
    | 'requirements'
    | 'rollback'
    | 'finalizing'
    | 'complete'
    | 'failed';

export type VersionActionType = 'Upgrade' | 'Downgrade' | 'Set';

export type VersionChangeProgress = {
    value: number;
    phase: VersionChangeProgressPhase;
    requirementsValue: number | null;
};

type ProgressLog = {
    message: string;
    finished?: boolean;
    error?: boolean;
};

// Updating requirements owns 50% of the overall bar. The surrounding 50% is
// split between preparing/checking out (20%), syncing files (25%), and finalizing (5%).
const REQUIREMENTS_START = 45;
const REQUIREMENTS_WEIGHT = 50;
const FINALIZING_START = REQUIREMENTS_START + REQUIREMENTS_WEIGHT;

const clampPercent = (value: number) => Math.max(0, Math.min(100, Math.round(value)));

const parseExplicitPercent = (message: string): number | null => {
    const match = message.match(/(?:^|\s)(\d{1,3}(?:\.\d+)?)\s*%/);
    if (!match) return null;
    const value = Number(match[1]);
    return Number.isFinite(value) ? clampPercent(value) : null;
};

const parseTransferPercent = (message: string): number | null => {
    const match = message.match(
        /(\d+(?:\.\d+)?)\s*\/\s*(\d+(?:\.\d+)?)\s*(?:kB|MB|GB|KiB|MiB|GiB)/i,
    );
    if (!match) return null;
    const transferred = Number(match[1]);
    const total = Number(match[2]);
    if (!Number.isFinite(transferred) || !Number.isFinite(total) || total <= 0) return null;
    return clampPercent((transferred / total) * 100);
};

const calculateRequirementsProgress = (
    message: string,
    previous: number,
    packageSignals: number,
): number => {
    const normalized = message.toLowerCase();
    const downloadPercent = parseExplicitPercent(message) ?? parseTransferPercent(message);
    let value = previous;

    if (normalized.includes('executing command:') && normalized.includes('pip install')) {
        value = Math.max(value, 5);
    }
    if (normalized.includes('looking in indexes')) value = Math.max(value, 8);
    if (normalized.includes('requirement already satisfied')) {
        value = Math.max(value, Math.min(65, 18 + packageSignals * 3));
    }
    if (normalized.includes('collecting ')) {
        value = Math.max(value, Math.min(38, 10 + packageSignals * 3));
    }
    if (normalized.includes('using cached')) value = Math.max(value, 45);
    if (normalized.includes('downloading ')) value = Math.max(value, 40);
    if (downloadPercent !== null) {
        // Download output can span many packages, so it advances only the middle
        // of the requirements segment rather than claiming the whole install.
        value = Math.max(value, 38 + downloadPercent * 0.32);
    }
    if (normalized.includes('building wheel') || normalized.includes('built wheel')) {
        value = Math.max(value, 70);
    }
    if (
        normalized.includes('installing collected packages')
        || normalized.includes('attempting uninstall')
    ) {
        value = Math.max(value, 78);
    }
    if (normalized.includes('successfully installed') && !normalized.includes('requirements from')) {
        value = Math.max(value, 96);
    }
    if (normalized.includes('successfully installed requirements from')) value = 100;

    return clampPercent(value);
};

export const calculateVersionChangeProgress = (
    logs: ProgressLog[],
    isProcessing: boolean,
): VersionChangeProgress => {
    let value = logs.length > 0 ? 2 : 0;
    let phase: VersionChangeProgressPhase = 'preparing';
    let requirementsValue: number | null = null;
    let requirementsStarted = false;
    let packageSignals = 0;
    let failed = false;
    let completed = false;

    for (const log of logs) {
        const message = log.message.trim();
        const normalized = message.toLowerCase();
        const isCheckoutMessage = normalized.includes('checked out commit')
            || normalized.includes('checked out previous commit');

        if (log.finished) {
            failed = !!log.error;
            completed = !log.error;
            continue;
        }

        if (isCheckoutMessage) {
            value = Math.max(value, 20);
            phase = 'checkout';
        }

        if (
            normalized.includes('syncing dependencies')
            || (normalized.includes('executing command:') && normalized.includes('pip install'))
        ) {
            requirementsStarted = true;
            requirementsValue ??= 0;
            value = Math.max(value, REQUIREMENTS_START);
            phase = 'requirements';
        }

        if (requirementsStarted) {
            if (
                normalized.includes('collecting ')
                || normalized.includes('requirement already satisfied')
                || normalized.includes('downloading ')
                || normalized.includes('using cached')
            ) {
                packageSignals += 1;
            }
            requirementsValue = calculateRequirementsProgress(
                message,
                requirementsValue ?? 0,
                packageSignals,
            );
            value = Math.max(
                value,
                REQUIREMENTS_START + requirementsValue * (REQUIREMENTS_WEIGHT / 100),
            );
            phase = 'requirements';
        }

        if (normalized.includes('requirements are up to date. skipping dependency sync')) {
            requirementsStarted = true;
            requirementsValue = 100;
            value = Math.max(value, FINALIZING_START);
            phase = 'finalizing';
        }

        if (normalized.includes('rolling back') || normalized.includes('rollback complete')) {
            phase = 'rollback';
        }

        if (
            (normalized.includes('updated ') && normalized.includes(' to version '))
            || (normalized.includes('upgraded ') && normalized.includes(' to version '))
        ) {
            value = Math.max(value, 98);
            phase = 'finalizing';
        } else if (!requirementsStarted && value >= 20 && !isCheckoutMessage) {
            value = Math.max(value, 30);
            phase = 'files';
        }
    }

    if (completed) {
        return {value: 100, phase: 'complete', requirementsValue};
    }
    if (failed) {
        return {value: clampPercent(value), phase: 'failed', requirementsValue};
    }

    // Keep an active operation below 100 until its explicit finish event arrives.
    return {
        value: Math.min(99, clampPercent(value)),
        phase: isProcessing ? phase : (value >= FINALIZING_START ? 'finalizing' : phase),
        requirementsValue,
    };
};
