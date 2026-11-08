// src/App.tsx
import './i18n';
import {useCallback, useEffect, useMemo, useRef, useState} from "react";
import {invoke} from "@tauri-apps/api/core";
import {listen, UnlistenFn} from "@tauri-apps/api/event";
import {getVersion} from '@tauri-apps/api/app';
import {getCurrentWindow} from '@tauri-apps/api/window';
import {openUrl} from '@tauri-apps/plugin-opener';
import ConsolePage from "./ConsolePage.tsx";
import SettingsPage from "./SettingsPage.tsx";
import AppCard from "./AppCard.tsx";
import ProfileSelectPage from "./ProfileSelectPage.tsx";
import {useConsoleLogs} from "./useConsoleLogs.ts";
import {createAppTheme, loadThemeMode, saveThemeMode, type ThemeModeSetting} from "./theme.ts";
import {
    compareVersions,
    getVersionActionProgressKey,
    getVersionActionType,
    isReleaseVersion,
} from "./versions.ts";
import {
    type AppInfo,
    type InlineConsoleKind,
    type InlineUpdateLogState,
    pickInitialProfile,
    type StatusState,
    type VersionChangeRequest,
    withoutKey,
} from "./types.ts";

import {
    Alert,
    Box,
    Button,
    Card,
    CardContent,
    CircularProgress,
    Container,
    Dialog,
    DialogActions,
    DialogContent,
    DialogContentText,
    DialogTitle,
    Link,
    Snackbar,
    Typography
} from "@mui/material";
import {ThemeProvider} from '@mui/material/styles';
import CssBaseline from '@mui/material/CssBaseline';
import useMediaQuery from '@mui/material/useMediaQuery';
import {useTranslation} from 'react-i18next';
import {invokeTauriCommandWrapper} from "./utils.ts";

type Page =
    'list'
    | 'installConsole'
    | 'runningAppConsole'
    | 'settings'
    | 'profileChooser'
    | 'changeProfile'
    | 'profileChangeConsole';

function App() {
    const {t} = useTranslation();
    const [app, setApp] = useState<AppInfo | null>(null);
    const [status, setStatus] = useState<StatusState>({loading: true, error: null, info: null, messageLoading: false});
    const [appActionLoading, setAppActionLoading] = useState<Record<string, boolean>>({});
    const [selectedTargetVersions, setSelectedTargetVersions] = useState<Record<string, string>>({});
    const selectedTargetVersionsRef = useRef(selectedTargetVersions);
    const [currentPage, setCurrentPage] = useState<Page>('list');
    // Console state remains keyed by app name so persisted logs stay compatible.
    const [inlineUpdateLogs, setInlineUpdateLogs] = useState<Record<string, InlineUpdateLogState>>({});
    const [inlineConsoles, setInlineConsoles] = useState<Record<string, InlineConsoleKind>>({});
    // Updated synchronously so the app event listener sees it before React re-renders.
    const completedAppsRef = useRef<Set<string>>(new Set());
    const appUpdateStatesRef = useRef<Record<string, AppInfo['update_state']>>({});
    const activeUpdateAppsRef = useRef<Set<string>>(new Set());
    const [isInstallProcessRunning, setIsInstallProcessRunning] = useState<boolean>(false);
    const [isStartAppProcessRunning, setIsStartAppProcessRunning] = useState<boolean>(false);
    const [startingAppName, setStartingAppName] = useState<string | null>(null);
    const {consoleLogs, addConsoleLog, beginConsoleSession, ensureActiveConsoleSession, seedConsoleSession} = useConsoleLogs();
    const [isRunningAppConsoleOpen, setIsRunningAppConsoleOpen] = useState<boolean>(false);
    const [themeMode, setThemeMode] = useState<ThemeModeSetting>(loadThemeMode);
    const [profileChoiceApp, setProfileChoiceApp] = useState<AppInfo | null>(null);
    const [selectedProfileForInstall, setSelectedProfileForInstall] = useState<string>("");
    const [appForProfileChange, setAppForProfileChange] = useState<AppInfo | null>(null);
    const [selectedNewProfileName, setSelectedNewProfileName] = useState<string>("");
    const [isProfileChangeProcessRunning, setIsProfileChangeProcessRunning] = useState<boolean>(false);
    const [profileChangeData, setProfileChangeData] = useState<{ appName: string; newProfile: string } | null>(null);
    const [isConfirmDeleteDialogOpen, setConfirmDeleteDialogOpen] = useState(false);
    const [appToDelete, setAppToDelete] = useState<string | null>(null);
    const [checkingUpdateForApp, setCheckingUpdateForApp] = useState<string | null>(null);
    const [appVersion, setAppVersion] = useState('');
    const [hiddenDefenderButtons, setHiddenDefenderButtons] = useState<Set<string>>(new Set());
    const [addingDefenderExclusionForApp, setAddingDefenderExclusionForApp] = useState<string | null>(null);
    const [snackbarOpen, setSnackbarOpen] = useState(false);
    const [snackbarMessage, setSnackbarMessage] = useState("");
    const [snackbarSeverity, setSnackbarSeverity] = useState<"success" | "info" | "warning" | "error">("info");

    const setActionLoading = useCallback((appName: string, loading: boolean) => {
        setAppActionLoading(prev => ({...prev, [appName]: loading}));
    }, []);

    const handleOpenWebsite = useCallback(async () => {
        const website = app?.website?.trim();
        if (!website) return;
        try {
            await openUrl(website);
        } catch (error) {
            console.warn('Failed to open app website:', error);
        }
    }, [app?.website]);

    useEffect(() => {
        selectedTargetVersionsRef.current = selectedTargetVersions;
    }, [selectedTargetVersions]);

    useEffect(() => {
        saveThemeMode(themeMode);
    }, [themeMode]);

    useEffect(() => {
        if (!app?.name) return;
        const launcherName = t('appLauncherName', {appName: app.name});
        document.title = launcherName;
        getCurrentWindow().setTitle(launcherName).catch((error) => {
            console.warn('Failed to update launcher window title:', error);
        });
    }, [app?.name, t]);

    const prefersDarkMode = useMediaQuery('(prefers-color-scheme: dark)');
    const muiTheme = useMemo(() => createAppTheme(themeMode, prefersDarkMode), [themeMode, prefersDarkMode]);

    const updateStatus = useCallback((newStatus: Partial<StatusState>) => {
        setStatus(prevStatus => ({...prevStatus, ...newStatus}));
    }, []);

    const clearMessages = useCallback(() => {
        updateStatus({error: null, info: null});
    }, [updateStatus]);

    const openProfileChooser = useCallback((appToInstall: AppInfo) => {
        setProfileChoiceApp(appToInstall);
        setSelectedProfileForInstall(pickInitialProfile(appToInstall, "default"));
        setCurrentPage('profileChooser');
    }, []);

    const handleAppPreferenceChange = async (
        appName: string,
        updateMethod: string | null,
        autoStart: boolean | null,
    ) => {
        clearMessages();
        updateStatus({messageLoading: true});
        await invokeTauriCommandWrapper<void>(
            'update_app_preferences',
            {appName, updateMethod, autoStart},
            () => updateStatus({info: t('App settings updated successfully.'), messageLoading: false}),
            (errorMessage) => updateStatus({error: `Failed to update ${appName}: ${errorMessage}`, messageLoading: false}),
        );
    };

    const handleStartApp = async (appName: string) => {
        clearMessages();
        setActionLoading(appName, true);
        setStartingAppName(appName);
        beginConsoleSession(appName, `Attempting to start app: ${appName}...`);
        setIsStartAppProcessRunning(true);
        setInlineConsoles(prev => ({...prev, [appName]: 'start'}));

        await invokeTauriCommandWrapper<void>("start_app", {appName}, () => {},
            (errorMessage, rawError) => {
                console.error(`Failed to start app ${appName}:`, rawError);
                addConsoleLog({message: `ERROR: Failed to start app: ${errorMessage}`, app_name: appName, error: true, finished: true});
                setIsStartAppProcessRunning(false);
            }
        );
    };

    const handleInstallWithProfile = async (appName: string, profileName: string) => {
        clearMessages();
        setActionLoading(appName, true);
        setStartingAppName(appName);
        beginConsoleSession(appName, `Initiating install for '${appName}' with profile '${profileName}'...`);
        setIsInstallProcessRunning(true);
        setCurrentPage('installConsole');

        await invokeTauriCommandWrapper<void>("setup_app", {appName, profileName}, () => {},
            (errorMessage, rawError) => {
                console.error(`Failed to invoke setup_app for ${appName} with profile ${profileName}:`, rawError);
                addConsoleLog({message: `ERROR: Failed to install app: ${errorMessage}`, app_name: appName, error: true, finished: true});
                setIsInstallProcessRunning(false);
            }
        );
    };

    const handleInstallClick = (appToInstall: AppInfo) => {
        if (appToInstall.profiles && appToInstall.profiles.length > 1) {
            openProfileChooser(appToInstall);
        } else {
            const profileName = appToInstall.current_profile || appToInstall.profiles?.[0]?.name || "default";
            handleInstallWithProfile(appToInstall.name, profileName);
        }
    };

    useEffect(() => {
        getVersion().then(setAppVersion);
    }, []);

    useEffect(() => {
        const unlistenPromises: Promise<UnlistenFn>[] = [];
        invoke('show_main_window').then();

        unlistenPromises.push(listen<AppInfo>("app", (event) => {
            const app = event.payload;
            const previousUpdateState = appUpdateStatesRef.current[app.name];
            if (app.update_state === 'updating' && previousUpdateState !== 'updating') {
                activeUpdateAppsRef.current.add(app.name);
                setInlineConsoles(prev => ({...prev, [app.name]: 'update'}));
                const actionType = getVersionActionType(
                    app.update_target_version ?? '',
                    app.current_version,
                    'Upgrade',
                );
                ensureActiveConsoleSession(
                    app.name,
                    `${getVersionActionProgressKey(actionType)} '${app.name}' to version '${app.update_target_version ?? 'unknown'}'`,
                );
            }
            appUpdateStatesRef.current[app.name] = app.update_state;
            setApp(app);
            const newSelectedTargets: Record<string, string> = {};
            const inlineLogUpdates: Record<string, InlineUpdateLogState> = {};
            if (app.update_state !== 'idle' && app.update_target_version) {
                const actionType = getVersionActionType(
                    app.update_target_version,
                    app.current_version,
                    'Upgrade',
                );
                newSelectedTargets[app.name] = app.update_target_version;
                inlineLogUpdates[app.name] = {
                    version: app.update_target_version,
                    actionType,
                    isConfirming: app.update_state === 'updating',
                    completed: false,
                    failed: app.update_state === 'failed',
                };
            } else if (!app.installed || app.running) {
                if (selectedTargetVersionsRef.current[app.name]) newSelectedTargets[app.name] = '';
            } else if (!completedAppsRef.current.has(app.name)) {
                const sortedVersions = [...app.available_versions]
                    .filter(isReleaseVersion)
                    .sort((a, b) => compareVersions(b, a));
                const latestVersion = sortedVersions[0];
                const currentSelection = selectedTargetVersionsRef.current[app.name];
                if (app.current_version && latestVersion && compareVersions(latestVersion, app.current_version) > 0) {
                    newSelectedTargets[app.name] = latestVersion;
                    // Auto-select latest version: show update log inline
                    inlineLogUpdates[app.name] = {
                        version: latestVersion,
                        actionType: 'Upgrade',
                        isConfirming: false,
                    };
                } else if (currentSelection && app.available_versions.includes(currentSelection) && currentSelection !== app.current_version) {
                    newSelectedTargets[app.name] = currentSelection;
                } else {
                    newSelectedTargets[app.name] = '';
                }
            }
            setSelectedTargetVersions(prev => ({...prev, ...newSelectedTargets}));
            // Preserve the result of the most recent attempt while app data refreshes.
            setInlineUpdateLogs(prev => {
                const merged = {...prev};
                for (const [name, entry] of Object.entries(inlineLogUpdates)) {
                    if (entry.isConfirming || entry.failed) {
                        completedAppsRef.current.delete(name);
                        merged[name] = entry;
                    } else if (!completedAppsRef.current.has(name) && !merged[name]?.failed) {
                        merged[name] = entry;
                    }
                }
                return merged;
            });
            updateStatus({loading: false});
        }));

        unlistenPromises.push(listen<AppInfo>("choose_app_profile", (event) => {
            openProfileChooser(event.payload);
        }));
        unlistenPromises.push(listen<{
            app_name: string;
            message: string;
            finished?: boolean;
            error?: boolean;
        }>("app-log", (event) => {
            const {app_name, finished, error} = event.payload;
            addConsoleLog(event.payload);
            const isUpdateEvent = activeUpdateAppsRef.current.has(app_name);
            if (isUpdateEvent) {
                setInlineUpdateLogs(prev => {
                    const entry = prev[app_name];
                    if (!entry || entry.completed) return prev;
                    if (finished) {
                        if (error) {
                            completedAppsRef.current.delete(app_name); // failed — not completed
                            return {...prev, [app_name]: {...entry, isConfirming: false, failed: true}};
                        } else {
                            completedAppsRef.current.add(app_name); // mark completed immediately
                            return {...prev, [app_name]: {...entry, isConfirming: false, completed: true, failed: false}};
                        }
                    } else if (!entry.isConfirming) {
                        return {...prev, [app_name]: {...entry, isConfirming: true, failed: false}};
                    }
                    return prev;
                });
                if (finished && !error) {
                    setSelectedTargetVersions(prev => ({...prev, [app_name]: ''}));
                }
                if (finished) activeUpdateAppsRef.current.delete(app_name);
            }
            if (finished) {
                setActionLoading(app_name, false);
                resetConsoleStates();
            }
        }));

        (async () => {
            await invokeTauriCommandWrapper<AppInfo>("load_app", undefined, () => {},
                (errorMessage, rawError) => {
                    console.error("Failed to initially load app:", rawError);
                    updateStatus({error: `Failed to load app: ${errorMessage}`, loading: false});
                }
            );
        })();

        return () => {
            Promise.all(unlistenPromises).then(unlisteners => unlisteners.forEach(fn => fn()));
        };
    }, [addConsoleLog, ensureActiveConsoleSession, openProfileChooser, setActionLoading, updateStatus]);

    const handleDeleteApp = async (appName: string) => {
        clearMessages();
        updateStatus({messageLoading: true});
        setActionLoading(appName, true);

        await invokeTauriCommandWrapper<void>("delete_app", {appName}, () => {},
            (errorMessage, rawError) => {
                console.error(`Failed to delete app ${appName}:`, rawError);
                updateStatus({error: `Delete app ${appName} failed: ${errorMessage}`});
            }
        );
        updateStatus({messageLoading: false});
        setActionLoading(appName, false);
    };

    const handleDeleteClick = (appName: string) => {
        setAppToDelete(appName);
        setConfirmDeleteDialogOpen(true);
    };

    const handleConfirmDelete = () => {
        if (appToDelete) handleDeleteApp(appToDelete);
        setAppToDelete(null);
        setConfirmDeleteDialogOpen(false);
    };

    const handleCancelDelete = () => {
        setAppToDelete(null);
        setConfirmDeleteDialogOpen(false);
    };

    const handleStopApp = async (appName: string) => {
        clearMessages();
        updateStatus({messageLoading: true});
        setActionLoading(appName, true);
        await invokeTauriCommandWrapper<void>("stop_app", {appName}, () => {},
            (errorMessage, rawError) => {
                console.error(`Failed to stop app ${appName}:`, rawError);
                updateStatus({error: `Stop app ${appName} failed: ${errorMessage}`});
            }
        );
        updateStatus({messageLoading: false});
        setActionLoading(appName, false);
    };

    const handleVersionSelected = (appName: string, targetVersion: string, currentAppVersion: string | null) => {
        setSelectedTargetVersions(p => ({...p, [appName]: targetVersion}));
        if (!targetVersion) {
            // Version cleared — hide inline log
            setInlineUpdateLogs(prev => withoutKey(prev, appName));
            return;
        }
        const actionType = getVersionActionType(targetVersion, currentAppVersion);
        setInlineUpdateLogs(prev => ({...prev, [appName]: {version: targetVersion, actionType, isConfirming: false}}));
    };

    const handleCancelVersionChange = (appName: string) => {
        completedAppsRef.current.delete(appName);
        setSelectedTargetVersions(p => ({...p, [appName]: ''}));
        setInlineUpdateLogs(prev => withoutKey(prev, appName));
    };

    const handleConfirmVersionChange = async (params: VersionChangeRequest) => {
        clearMessages();
        activeUpdateAppsRef.current.add(params.appName);
        setActionLoading(params.appName, true);
        // Mark as confirming so the inline log shows a spinner
        setInlineUpdateLogs(prev => ({
            ...prev,
            [params.appName]: {...(prev[params.appName] ?? {version: params.version, actionType: params.actionType}), isConfirming: true, completed: false, failed: false}
        }));
        setStartingAppName(params.appName);
        beginConsoleSession(params.appName, `Initiating ${params.actionType} for '${params.appName}' to version '${params.version}'...`);
        setInlineConsoles(prev => ({...prev, [params.appName]: 'update'}));

        await invokeTauriCommandWrapper<void>("update_to_version", {appName: params.appName, version: params.version}, () => {},
            (errorMessage, rawError) => {
                console.error(`Failed to invoke ${params.actionType.toLowerCase()}:`, rawError);
                const operationError = `Upgrade failed: ${errorMessage}`;
                completedAppsRef.current.delete(params.appName);
                setInlineUpdateLogs(prev => {
                    const entry = prev[params.appName];
                    if (!entry) return prev;
                    return {...prev, [params.appName]: {...entry, isConfirming: false, completed: false, failed: true}};
                });
                addConsoleLog({message: operationError, app_name: params.appName, error: true, finished: true});
            }
        );
    };

    const handleOpenRunningAppConsole = (appName: string) => {
        clearMessages();
        setStartingAppName(appName);
        const consoleTitle = (app?.running && !app.installed) ? `Installation console for: ${appName}` : `Console for running app: ${appName}`;
        seedConsoleSession({message: consoleTitle, app_name: appName});
        setIsRunningAppConsoleOpen(true);
        setCurrentPage('runningAppConsole');
    };

    const handleOpenUpdateConsole = (appName: string) => {
        const entry = inlineUpdateLogs[appName];
        const version = app?.update_target_version ?? entry?.version;
        if (!version) return;
        const actionType = entry?.actionType ?? 'Upgrade';
        setStartingAppName(appName);
        seedConsoleSession({
            message: app?.update_error ?? `${actionType} to ${version}`,
            app_name: appName,
            error: app?.update_state === 'failed',
            finished: app?.update_state === 'failed',
        });
        setInlineConsoles(prev => ({...prev, [appName]: 'update'}));
    };

    const reloadApp = (errorPrefix: string, onSuccess?: () => void) =>
        invokeTauriCommandWrapper<AppInfo>("load_app", undefined, () => onSuccess?.(),
            (errorMessage, rawError) => {
                console.error(`${errorPrefix}:`, rawError);
                updateStatus({error: `${errorPrefix}: ${errorMessage}`, loading: false});
            }
        );

    const handleCloseInlineConsole = async (appName: string) => {
        setInlineConsoles(prev => withoutKey(prev, appName));
        setActionLoading(appName, false);
        if (startingAppName === appName) {
            setStartingAppName(null);
            setIsStartAppProcessRunning(false);
        }
        await reloadApp("Failed to reload app");
    };

    function resetConsoleStates() {
        setIsInstallProcessRunning(false);
        setIsStartAppProcessRunning(false);
        setIsRunningAppConsoleOpen(false);
        setIsProfileChangeProcessRunning(false);
    }

    const handleBackFromConsole = async () => {
        setCurrentPage('list');
        resetConsoleStates();
        clearMessages();
        updateStatus({messageLoading: false});
        if (startingAppName) setActionLoading(startingAppName, false);
        setStartingAppName(null);
        setProfileChangeData(null);

        updateStatus({loading: true, info: t("Refreshing app...")});
        await reloadApp("Failed to reload app", () => updateStatus({loading: false, info: t("App Refreshed.")}));
    };

    const handleNavigateToChangeProfilePage = (appToChange: AppInfo) => {
        clearMessages();
        setAppForProfileChange(appToChange);
        setSelectedNewProfileName(pickInitialProfile(appToChange, ""));
        setCurrentPage('changeProfile');
    };

    const handleConfirmProfileChange = async (appName: string, newProfileName: string) => {
        clearMessages();
        setActionLoading(appName, true);
        setStartingAppName(appName);
        setProfileChangeData({appName, newProfile: newProfileName});
        beginConsoleSession(appName, `Initiating profile change for '${appName}' to '${newProfileName}'...`);
        setIsProfileChangeProcessRunning(true);
        setCurrentPage('profileChangeConsole');

        await invokeTauriCommandWrapper<void>("setup_app", {appName, profileName: newProfileName}, () => {},
            (errorMessage, rawError) => {
                console.error(`Failed to invoke setup_app for profile change:`, rawError);
                addConsoleLog({message: `ERROR: Failed to change profile: ${errorMessage}`, app_name: appName, error: true, finished: true});
                setIsProfileChangeProcessRunning(false);
            }
        );
    };

    useEffect(() => {
        const shouldShow = (status.info || status.error) && !status.messageLoading &&
            ['list', 'settings', 'changeProfile'].includes(currentPage);
        if (shouldShow) {
            setSnackbarMessage(status.error || status.info || "");
            setSnackbarSeverity(status.error ? "error" : "info");
            setSnackbarOpen(true);
            const timerId = setTimeout(() => updateStatus({error: null, info: null}), status.error ? 8000 : 5000);
            return () => clearTimeout(timerId);
        }
        if (!status.info && !status.error) setSnackbarOpen(false);
    }, [status.info, status.error, status.messageLoading, updateStatus, currentPage]);

    const handleCheckForUpdates = async (appName: string) => {
        clearMessages();
        setActionLoading(appName, true);
        setCheckingUpdateForApp(appName);
        await invokeTauriCommandWrapper<void>("load_app", undefined,
            () => updateStatus({info: t("App Refreshed.")}),
            (errorMessage, rawError) => {
                console.error("Failed to check for updates:", rawError);
                updateStatus({error: `Failed to check for updates: ${errorMessage}`});
            }
        );
        setActionLoading(appName, false);
        setCheckingUpdateForApp(null);
    };

    const handleAddDefenderExclusion = async (appName: string) => {
        clearMessages();
        setActionLoading(appName, true);
        setAddingDefenderExclusionForApp(appName);
        await invokeTauriCommandWrapper<void>("add_defender_exclusion", {appName},
            () => {
                updateStatus({info: t('defenderExclusionAdded', {appName})});
                setHiddenDefenderButtons(prev => new Set(prev).add(appName));
            },
            (errorMessage, rawError) => {
                console.error(`Failed to add defender exclusion for ${appName}:`, rawError);
                updateStatus({error: t('failedToAddExclusion', {errorMessage})});
            }
        );
        setAddingDefenderExclusionForApp(null);
        setActionLoading(appName, false);
    };

    const isInlineConsoleVisible = currentPage === 'list' && !!app && !!inlineConsoles[app.name];
    const consolePage = (title: string, appName: string, isProcessing: boolean) => (
        <ConsolePage title={title} appName={appName} logs={consoleLogs[appName] ?? []} onBack={handleBackFromConsole} isProcessing={isProcessing}/>
    );
    let pageContent;

    if (currentPage === 'installConsole' && startingAppName) {
        pageContent = consolePage(t('Installing App: {{appName}}', {appName: startingAppName}), startingAppName, isInstallProcessRunning);
    } else if (currentPage === 'runningAppConsole' && startingAppName) {
        pageContent = consolePage(t('Console: {{appName}}', {appName: startingAppName}), startingAppName, isRunningAppConsoleOpen);
    } else if (currentPage === 'profileChangeConsole' && profileChangeData && startingAppName) {
        pageContent = consolePage(
            t("Changing Profile: {{appName}} to '{{newProfile}}'", {appName: profileChangeData.appName, newProfile: profileChangeData.newProfile}),
            startingAppName,
            isProfileChangeProcessRunning,
        );
    } else if (currentPage === 'settings') {
        pageContent = <SettingsPage currentTheme={themeMode} onChangeTheme={setThemeMode} onBack={() => setCurrentPage('list')} updateStatus={updateStatus} clearMessages={clearMessages} />;
    } else if (currentPage === 'profileChooser' && profileChoiceApp) {
        pageContent = (
            <ProfileSelectPage
                app={profileChoiceApp}
                title={t('Choose Profile for {{appName}}', {appName: profileChoiceApp.name})}
                selectLabel={t('Profile')}
                selectedProfile={selectedProfileForInstall}
                onSelectProfile={setSelectedProfileForInstall}
                confirmLabel={t("Confirm & Install")}
                busyLabel={t("Starting Install...")}
                isBusy={!!appActionLoading[profileChoiceApp.name]}
                onConfirm={() => handleInstallWithProfile(profileChoiceApp.name, selectedProfileForInstall)}
                onCancel={() => setCurrentPage('list')}
                emptyBackLabel={t('Back')}
            />
        );
    } else if (currentPage === 'changeProfile' && appForProfileChange) {
        pageContent = (
            <ProfileSelectPage
                app={appForProfileChange}
                title={t('Change Profile for {{appName}}', {appName: appForProfileChange.name})}
                subtitle={t('Current Profile: {{profile}}', {profile: appForProfileChange.current_profile})}
                selectLabel={t('New Profile')}
                selectedProfile={selectedNewProfileName}
                onSelectProfile={setSelectedNewProfileName}
                currentProfile={appForProfileChange.current_profile}
                confirmLabel={t("Change Profile")}
                busyLabel={t("Initiating...")}
                isBusy={!!appActionLoading[appForProfileChange.name]}
                onConfirm={() => handleConfirmProfileChange(appForProfileChange.name, selectedNewProfileName)}
                onCancel={() => setCurrentPage('list')}
            />
        );
    } else {
        pageContent = (
            <Container maxWidth="lg" sx={{
                py: isInlineConsoleVisible ? 2 : {xs: 2, md: 4},
                height: isInlineConsoleVisible ? '100vh' : 'auto',
                boxSizing: 'border-box',
                overflow: isInlineConsoleVisible ? 'hidden' : 'visible',
            }}>
                <Snackbar open={snackbarOpen} autoHideDuration={6000} onClose={() => setSnackbarOpen(false)} anchorOrigin={{vertical: 'bottom', horizontal: 'center'}}>
                    <Alert onClose={() => setSnackbarOpen(false)} severity={snackbarSeverity} sx={{width: '100%'}}>{snackbarMessage}</Alert>
                </Snackbar>
                {status.loading && !app && (
                    <Card variant="outlined" sx={{borderColor: 'divider', boxShadow: 'none'}}>
                        <CardContent sx={{display: 'flex', justifyContent: 'center', alignItems: 'center', minHeight: 220}}>
                            <CircularProgress size={24}/><Typography sx={{ml: 1.5}} color="text.secondary">{t('Loading app...')}</Typography>
                        </CardContent>
                    </Card>
                )}
                {!status.loading && !app && (
                    <Card variant="outlined" sx={{borderColor: 'divider', boxShadow: 'none'}}>
                        <CardContent sx={{py: 8, textAlign: 'center'}}><Typography color="text.secondary">{t('App configuration could not be loaded.')}</Typography></CardContent>
                    </Card>
                )}
                {app && (
                    <AppCard
                        key={app.name}
                        app={app}
                        isBusy={currentPage !== 'list' || !!status.messageLoading}
                        isActionLoading={!!appActionLoading[app.name]}
                        inlineConsoleKind={inlineConsoles[app.name]}
                        inlineUpdateEntry={inlineUpdateLogs[app.name]}
                        logs={consoleLogs[app.name] ?? []}
                        isStartProcessing={isStartAppProcessRunning && startingAppName === app.name}
                        showDefenderButton={!hiddenDefenderButtons.has(app.name)}
                        isAddingDefenderExclusion={addingDefenderExclusionForApp === app.name}
                        isCheckingUpdates={checkingUpdateForApp === app.name}
                        selectedTargetVersion={selectedTargetVersions[app.name] || ''}
                        onOpenWebsite={handleOpenWebsite}
                        onPreferenceChange={(updateMethod, autoStart) => handleAppPreferenceChange(app.name, updateMethod, autoStart)}
                        onStart={() => handleStartApp(app.name)}
                        onStop={() => handleStopApp(app.name)}
                        onOpenConsole={() => handleOpenRunningAppConsole(app.name)}
                        onInstall={() => handleInstallClick(app)}
                        onAddDefenderExclusion={() => handleAddDefenderExclusion(app.name)}
                        onChangeProfile={() => handleNavigateToChangeProfilePage(app)}
                        onDelete={() => handleDeleteClick(app.name)}
                        onOpenSettings={() => setCurrentPage('settings')}
                        onCheckForUpdates={() => handleCheckForUpdates(app.name)}
                        onSelectVersion={(version) => handleVersionSelected(app.name, version, app.current_version)}
                        onCloseInlineConsole={() => handleCloseInlineConsole(app.name)}
                        onConfirmVersionChange={handleConfirmVersionChange}
                        onOpenUpdateConsole={() => handleOpenUpdateConsole(app.name)}
                        onCancelVersionChange={() => handleCancelVersionChange(app.name)}
                    />
                )}
                <Dialog open={isConfirmDeleteDialogOpen} onClose={handleCancelDelete}>
                    <DialogTitle>{t('Confirm Deletion')}</DialogTitle>
                    <DialogContent><DialogContentText>{appToDelete && t('Are you sure you want to delete {{appName}}?', {appName: appToDelete})}</DialogContentText></DialogContent>
                    <DialogActions><Button onClick={handleCancelDelete}>{t('Cancel')}</Button><Button onClick={handleConfirmDelete} color="error" autoFocus>{t('Delete')}</Button></DialogActions>
                </Dialog>
            </Container>
        );
    }
    return (
        <ThemeProvider theme={muiTheme}>
            <CssBaseline/>
            <Box sx={{display: 'flex', flexDirection: 'column', minHeight: '100vh'}}>
                <Box component="main" sx={{flex: '1 1 auto'}}>{pageContent}</Box>
                {currentPage === 'list' && !isInlineConsoleVisible && (
                    <Box component="footer" sx={{py: 2, textAlign: 'center'}}>
                        <Typography variant="body2" color="text.secondary">
                            <Link href="https://github.com/ok-oldking/pyappify" target="_blank" rel="noopener noreferrer">{t('appMadeWith', {name: `PyAppify ${appVersion}`})}</Link>
                        </Typography>
                    </Box>
                )}
            </Box>
        </ThemeProvider>
    );
}
export default App;
