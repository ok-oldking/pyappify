// src/App.tsx
import './i18n';
import {useCallback, useEffect, useMemo, useRef, useState} from "react";
import {invoke} from "@tauri-apps/api/core";
import {listen, UnlistenFn} from "@tauri-apps/api/event";
import {getVersion} from '@tauri-apps/api/app';
import UpdateLogPage from "./UpdateLogPage";
import ConsolePage from "./ConsolePage.tsx";
import SettingsPage from "./SettingsPage.tsx";

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
    FormControl,
    IconButton,
    InputLabel,
    Link,
    List,
    ListItem,
    MenuItem,
    Select,
    SelectChangeEvent,
    Snackbar,
    Stack,
    Tooltip,
    Typography
} from "@mui/material";
import {
    ArrowDownward,
    Build,
    Cached,
    Delete,
    OpenInNew,
    PlayArrow,
    Settings as SettingsIcon,
    SettingsApplications,
    StopCircle,
    Update
} from '@mui/icons-material';
import {createTheme, ThemeProvider} from '@mui/material/styles';
import CssBaseline from '@mui/material/CssBaseline';
import useMediaQuery from '@mui/material/useMediaQuery';
import {useTranslation} from 'react-i18next';

interface Profile {
    name: string;
    main_script: string;
    admin: boolean;
    requirements: string;
    python_path: string;
}

interface App {
    name: string;
    url: string;
    path: string;
    current_version: string | null;
    available_versions: string[];
    running: boolean;
    installed: boolean;
    profiles: Profile[];
    current_profile: string;
}

interface ConfigItemFromRust {
    name: string;
    description: string;
    value: string | number;
    default_value: string | number;
    options?: (string | number)[];
}

const compareVersions = (v1: string, v2: string): number => {
    return v1.localeCompare(v2, undefined, {numeric: true, sensitivity: 'base'});
};

type StatusState = {
    loading?: boolean;
    error?: string | null;
    info?: string | null;
    messageLoading?: boolean;
};

type Page =
    'list'
    | 'updateLog'
    | 'installConsole'
    | 'startConsole'
    | 'versionChangeConsole'
    | 'runningAppConsole'
    | 'settings'
    | 'profileChooser'
    | 'changeProfile'
    | 'profileChangeConsole';

type ThemeModeSetting = 'light' | 'dark' | 'system';

const PIP_CACHE_DIR_CONFIG_KEY = "Pip Cache Directory";


async function invokeTauriCommandWrapper<T>(
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

function App() {
    const {t} = useTranslation();
    const [apps, setApps] = useState<App[] | null>(null);
    const [status, setStatus] = useState<StatusState>({loading: true, error: null, info: null, messageLoading: false});

    const [appActionLoading, setAppActionLoading] = useState<Record<string, boolean>>({});

    const [selectedTargetVersions, setSelectedTargetVersions] = useState<Record<string, string>>({});
    const selectedTargetVersionsRef = useRef(selectedTargetVersions);
    useEffect(() => {
        selectedTargetVersionsRef.current = selectedTargetVersions;
    }, [selectedTargetVersions]);

    const [currentPage, setCurrentPage] = useState<Page>('list');
    const currentPageRef = useRef(currentPage);
    useEffect(() => {
        currentPageRef.current = currentPage;
    }, [currentPage]);

    const [updateLogViewData, setUpdateLogViewData] = useState<{
        name: string;
        version: string;
        actionType: string;
    } | null>(null);

    const [isInstallProcessRunning, setIsInstallProcessRunning] = useState<boolean>(false);
    const [isStartAppProcessRunning, setIsStartAppProcessRunning] = useState<boolean>(false);
    const [startingAppName, setStartingAppName] = useState<string | null>(null);
    const [consoleInitialMessage, setConsoleInitialMessage] = useState<string | undefined>(undefined);

    const [versionChangeConsoleData, setVersionChangeConsoleData] = useState<{
        appName: string;
        version: string;
        actionType: string;
    } | null>(null);
    const [isVersionChangeProcessRunning, setIsVersionChangeProcessRunning] = useState<boolean>(false);
    const [isRunningAppConsoleOpen, setIsRunningAppConsoleOpen] = useState<boolean>(false);

    const [themeMode, setThemeMode] = useState<ThemeModeSetting>(() => {
        const savedTheme = localStorage.getItem('appThemeMode');
        if (savedTheme === 'light' || savedTheme === 'dark' || savedTheme === 'system') {
            return savedTheme as ThemeModeSetting;
        }
        return 'system';
    });

    const [allConfigs, setAllConfigs] = useState<ConfigItemFromRust[] | null>(null);
    const [isLoadingConfigs, setIsLoadingConfigs] = useState<boolean>(true);

    const [profileChoiceApp, setProfileChoiceApp] = useState<App | null>(null);
    const [selectedProfileForInstall, setSelectedProfileForInstall] = useState<string>("");

    const [appForProfileChange, setAppForProfileChange] = useState<App | null>(null);
    const [selectedNewProfileName, setSelectedNewProfileName] = useState<string>("");
    const [isProfileChangeProcessRunning, setIsProfileChangeProcessRunning] = useState<boolean>(false);
    const [profileChangeData, setProfileChangeData] = useState<{ appName: string; newProfile: string } | null>(null);
    const initialAutoStartDoneRef = useRef(false);

    const [isConfirmDeleteDialogOpen, setConfirmDeleteDialogOpen] = useState(false);
    const [appToDelete, setAppToDelete] = useState<string | null>(null);
    const [checkingUpdateForApp, setCheckingUpdateForApp] = useState<string | null>(null);
    const [appVersion, setAppVersion] = useState('');


    useEffect(() => {
        localStorage.setItem('appThemeMode', themeMode);
    }, [themeMode]);

    const prefersDarkMode = useMediaQuery('(prefers-color-scheme: dark)');
    const muiTheme = useMemo(() => {
        let mode: 'light' | 'dark';
        if (themeMode === 'system') {
            mode = prefersDarkMode ? 'dark' : 'light';
        } else {
            mode = themeMode;
        }
        return createTheme({
            palette: {
                mode,
            },
        });
    }, [themeMode, prefersDarkMode]);


    const updateStatus = useCallback((newStatus: Partial<StatusState>) => {
        setStatus(prevStatus => ({...prevStatus, ...newStatus}));
    }, []);


    const clearMessages = useCallback(() => {
        updateStatus({error: null, info: null});
    }, [updateStatus]);


    const handleStartApp = async (appName: string) => {
        clearMessages();
        setAppActionLoading(prev => ({...prev, [appName]: true}));
        setStartingAppName(appName);
        setConsoleInitialMessage(`Attempting to start app: ${appName}...`);
        setIsStartAppProcessRunning(true);
        setCurrentPage('startConsole');

        await invokeTauriCommandWrapper<void>(
            "start_app",
            {appName},
            () => {
            },
            (errorMessage, rawError) => {
                console.error(`Failed to start app ${appName}:`, rawError);
                setConsoleInitialMessage(prev => `${prev}\nERROR (client-side): Failed to dispatch start operation: ${errorMessage}`);
            }
        );
    };

    const handleInstallWithProfile = async (appName: string, profileName: string) => {
        clearMessages();
        setAppActionLoading(prev => ({...prev, [appName]: true}));
        setStartingAppName(appName);
        setConsoleInitialMessage(`Initiating install for '${appName}' with profile '${profileName}'...`);
        setIsInstallProcessRunning(true);
        setCurrentPage('installConsole');

        await invokeTauriCommandWrapper<void>(
            "setup_app",
            {appName, profileName},
            () => {
            },
            (errorMessage, rawError) => {
                console.error(`Failed to invoke setup_app for ${appName} with profile ${profileName}:`, rawError);
                setConsoleInitialMessage(prev => `${prev}\nERROR (client-side): Failed to dispatch install operation: ${errorMessage}`);
            }
        );
    };

    const handleInstallClick = (app: App) => {
        if (app.profiles && app.profiles.length > 1) {
            setProfileChoiceApp(app);
            let initialProfile = app.current_profile;
            if (!app.profiles.find(p => p.name === initialProfile)) {
                initialProfile = app.profiles[0]?.name || "default";
            }
            setSelectedProfileForInstall(initialProfile);
            setCurrentPage('profileChooser');
        } else {
            const profileName = app.current_profile || app.profiles?.[0]?.name || "default";
            handleInstallWithProfile(app.name, profileName);
        }
    };

    useEffect(() => {
        getVersion().then(setAppVersion);
    }, []);

    useEffect(() => {
        const unlistenPromises: Promise<UnlistenFn>[] = [];

        invoke('show_main_window').then();

        unlistenPromises.push(listen<App[]>("apps", (event) => {
            console.log("Received apps event:", event);
            const newApps = event.payload;
            setApps(newApps);

            if (!initialAutoStartDoneRef.current && newApps && newApps.length > 0) {
                initialAutoStartDoneRef.current = true;
                const appToAutoStart = newApps.find(app => {
                    if (!app.installed || app.running || !app.current_version) {
                        return false;
                    }
                    const hasUpdate = app.available_versions.some(v => compareVersions(v, app.current_version!) > 0);
                    return !hasUpdate;
                });

                if (appToAutoStart) {
                    handleStartApp(appToAutoStart.name);
                }
            }

            const newSelectedTargets: Record<string, string> = {};
            newApps.forEach(app => {
                if (!app.installed || app.running) {
                    if (selectedTargetVersionsRef.current[app.name]) {
                        newSelectedTargets[app.name] = '';
                    }
                    return;
                }

                const sortedVersions = [...app.available_versions].sort((a, b) => compareVersions(b, a));
                const latestVersion = sortedVersions.length > 0 ? sortedVersions[0] : undefined;
                const currentExistingSelection = selectedTargetVersionsRef.current[app.name];

                if (app.current_version && latestVersion && compareVersions(latestVersion, app.current_version) > 0) {
                    newSelectedTargets[app.name] = latestVersion;
                } else if (currentExistingSelection && app.available_versions.includes(currentExistingSelection) && currentExistingSelection !== app.current_version) {
                    newSelectedTargets[app.name] = currentExistingSelection;
                } else if (!app.current_version && latestVersion) {
                    newSelectedTargets[app.name] = latestVersion;
                } else {
                    newSelectedTargets[app.name] = '';
                }
            });
            setSelectedTargetVersions(prev => ({...prev, ...newSelectedTargets}));
            updateStatus({loading: false});
        }));

        unlistenPromises.push(listen<App>("choose_app_profile", (event) => {
            console.log("Received choose_app_profile event:", event);
            const appForProfileChoice = event.payload;
            setProfileChoiceApp(appForProfileChoice);

            let initialProfile = appForProfileChoice.current_profile;
            if (!appForProfileChoice.profiles?.find(p => p.name === initialProfile)) {
                initialProfile = appForProfileChoice.profiles?.[0]?.name || "default";
            }
            setSelectedProfileForInstall(initialProfile);
            setCurrentPage('profileChooser');
        }));

        (async () => {
            await invokeTauriCommandWrapper<App[]>(
                "load_apps",
                undefined,
                () => {
                },
                (errorMessage, rawError) => {
                    console.error("Failed to initially load apps:", rawError);
                    updateStatus({error: `Failed to load apps: ${errorMessage}`, info: null, loading: false});
                }
            );
        })();

        const loadInitialConfigs = async () => {
            setIsLoadingConfigs(true);
            await invokeTauriCommandWrapper<ConfigItemFromRust[]>(
                'get_config_payload',
                undefined,
                (configs) => {
                    setAllConfigs(configs);
                },
                (errorMsg, rawError) => {
                    console.error("Failed to load initial configurations:", rawError);
                    if (currentPageRef.current === 'list' || currentPageRef.current === 'settings') {
                        updateStatus({error: `Failed to load settings: ${errorMsg}`});
                    }
                }
            );
            setIsLoadingConfigs(false);
        };
        loadInitialConfigs();


        return () => {
            Promise.all(unlistenPromises).then(unlisteners => {
                unlisteners.forEach(unlistenFn => unlistenFn());
            }).catch(err => console.error("Error during unlisten setup:", err));
        };
    }, [updateStatus]);

    const handleDeleteApp = async (appName: string) => {
        clearMessages();
        updateStatus({messageLoading: true});
        setAppActionLoading(prev => ({...prev, [appName]: true}));

        await invokeTauriCommandWrapper<void>(
            "delete_app",
            {appName},
            () => {
            },
            (errorMessage, rawError) => {
                console.error(`Failed to delete app ${appName}:`, rawError);
                updateStatus({error: `Delete app ${appName} failed: ${errorMessage}`});
            }
        );

        updateStatus({messageLoading: false});
        setAppActionLoading(prev => ({...prev, [appName]: false}));
    };

    const handleDeleteClick = (appName: string) => {
        setAppToDelete(appName);
        setConfirmDeleteDialogOpen(true);
    };

    const handleConfirmDelete = () => {
        if (appToDelete) {
            handleDeleteApp(appToDelete);
        }
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
        setAppActionLoading(prev => ({...prev, [appName]: true}));

        await invokeTauriCommandWrapper<void>(
            "stop_app",
            {appName},
            () => {
            },
            (errorMessage, rawError) => {
                console.error(`Failed to stop app ${appName}:`, rawError);
                updateStatus({error: `Stop app ${appName} failed: ${errorMessage}`});
            }
        );

        updateStatus({messageLoading: false});
        setAppActionLoading(prev => ({...prev, [appName]: false}));
    };

    const handleNavigateToUpdateLogPage = (appName: string, targetVersion: string | undefined, currentAppVersion: string | null) => {
        if (!targetVersion) {
            updateStatus({error: `Please select a version for ${appName}.`, info: null});
            return;
        }
        clearMessages();

        let actionType = "Set";
        if (currentAppVersion) {
            const comparison = compareVersions(targetVersion, currentAppVersion);
            if (comparison > 0) actionType = "Update";
            else if (comparison < 0) actionType = "Downgrade";
            else {
                updateStatus({error: `Selected version is the current version for ${appName}.`});
                return;
            }
        }

        setUpdateLogViewData({name: appName, version: targetVersion, actionType});
        setCurrentPage('updateLog');
    };

    const handleBackFromUpdateLog = () => {
        setCurrentPage('list');
        setUpdateLogViewData(null);
        if (updateLogViewData?.name && appActionLoading[updateLogViewData.name]) {
            setAppActionLoading(prev => ({...prev, [updateLogViewData.name!]: false}));
        }
    };

    const handleConfirmVersionChange = async (params: { appName: string, version: string, actionType: string }) => {
        const {appName, version, actionType} = params;
        clearMessages();
        setAppActionLoading(prev => ({...prev, [appName]: true}));
        setVersionChangeConsoleData(params);
        setStartingAppName(appName);
        setConsoleInitialMessage(`Initiating ${actionType} for '${appName}' to version '${version}'...`);
        setIsVersionChangeProcessRunning(true);
        setCurrentPage('versionChangeConsole');

        const app = apps?.find(a => a.name === appName);
        const requirementsFile = app?.profiles?.find(p => p.name === app.current_profile)?.requirements || "requirements.txt";

        await invokeTauriCommandWrapper<void>(
            "update_to_version",
            {appName, version, requirements: requirementsFile},
            () => {
            },
            (errorMessage, rawError) => {
                console.error(`Failed to invoke ${actionType.toLowerCase()} for ${appName} to version ${version}:`, rawError);
                setConsoleInitialMessage(prev => `${prev}\nERROR (client-side): Failed to dispatch ${actionType.toLowerCase()} operation: ${errorMessage}`);
            }
        );
    };

    const handleOpenRunningAppConsole = (appName: string) => {
        clearMessages();
        setStartingAppName(appName);
        const app = apps?.find(a => a.name === appName);
        const consoleTitleMessage = (app && app.running && !app.installed)
            ? `Installation console for: ${appName}`
            : `Console for running app: ${appName}`;
        setConsoleInitialMessage(consoleTitleMessage);
        setIsRunningAppConsoleOpen(true);
        setCurrentPage('runningAppConsole');
    };

    const handleBackFromRunningAppConsole = () => {
        setCurrentPage('list');
        resetConsoleStates();
        if (startingAppName) {
            setAppActionLoading(prev => ({...prev, [startingAppName]: false}));
        }
        setStartingAppName(null);
        clearMessages();
    };


    const resetConsoleStates = () => {
        setConsoleInitialMessage(undefined);
        if (isInstallProcessRunning) setIsInstallProcessRunning(false);
        if (isStartAppProcessRunning) setIsStartAppProcessRunning(false);
        if (isVersionChangeProcessRunning) setIsVersionChangeProcessRunning(false);
        if (isRunningAppConsoleOpen) setIsRunningAppConsoleOpen(false);
        if (isProfileChangeProcessRunning) setIsProfileChangeProcessRunning(false);
    }

    const handleBackFromInstallConsole = async () => {
        setCurrentPage('list');
        resetConsoleStates();
        clearMessages();
        updateStatus({messageLoading: false});
        setAppActionLoading(prev => ({
            ...prev,
            ...(startingAppName && {[startingAppName]: false})
        }));
        setStartingAppName(null);

        updateStatus({loading: true, info: t("Refreshing app...")});
        await invokeTauriCommandWrapper<App[]>(
            "load_apps",
            undefined,
            () => {
                updateStatus({loading: false, info: "App refreshed."});
            },
            (errorMessage, rawError) => {
                console.error("Failed to reload apps after install/clone attempt:", rawError);
                updateStatus({error: `Failed to reload apps: ${errorMessage}`, info: null, loading: false});
            }
        );
    };

    const handleBackFromStartConsole = async () => {
        setCurrentPage('list');
        resetConsoleStates();
        if (startingAppName) {
            setAppActionLoading(prev => ({...prev, [startingAppName]: false}));
        }
        setStartingAppName(null);
        clearMessages();
        updateStatus({messageLoading: false});

        updateStatus({loading: true, info: "Refreshing app..."});
        await invokeTauriCommandWrapper<App[]>(
            "load_apps",
            undefined,
            () => {
                updateStatus({loading: false, info: "App refreshed."});
            },
            (errorMessage, rawError) => {
                console.error("Failed to reload apps after start attempt:", rawError);
                updateStatus({error: `Failed to reload apps: ${errorMessage}`, info: null, loading: false});
            }
        );
    };

    const handleBackFromVersionChangeConsole = async () => {
        setCurrentPage('list');
        resetConsoleStates();
        if (startingAppName) {
            setAppActionLoading(prev => ({...prev, [startingAppName]: false}));
        }
        const appNameThatChanged = startingAppName;
        setStartingAppName(null);
        setVersionChangeConsoleData(null);
        clearMessages();
        updateStatus({messageLoading: false});

        updateStatus({loading: true, info: "Refreshing App..."});
        await invokeTauriCommandWrapper<App[]>(
            "load_apps",
            undefined,
            () => {
                updateStatus({loading: false, info: "App refreshed."});
                if (appNameThatChanged) {
                    setSelectedTargetVersions(prev => ({
                        ...prev,
                        [appNameThatChanged]: '',
                    }));
                }
            },
            (errorMessage, rawError) => {
                console.error("Failed to reload apps after version change attempt:", rawError);
                updateStatus({error: `Failed to reload apps: ${errorMessage}`, info: null, loading: false});
            }
        );
    };

    const handleNavigateToChangeProfilePage = (appToChange: App) => {
        clearMessages();
        setAppForProfileChange(appToChange);
        let initialProfile = appToChange.current_profile;
        if (!appToChange.profiles?.find(p => p.name === initialProfile)) {
            initialProfile = appToChange.profiles?.[0]?.name || "";
        }
        setSelectedNewProfileName(initialProfile);
        setCurrentPage('changeProfile');
    };

    const handleConfirmProfileChange = async (appName: string, newProfileName: string) => {
        clearMessages();
        setAppActionLoading(prev => ({...prev, [appName]: true}));
        setStartingAppName(appName);
        setProfileChangeData({appName, newProfile: newProfileName});
        setConsoleInitialMessage(`Initiating profile change for '${appName}' to '${newProfileName}'...`);
        setIsProfileChangeProcessRunning(true);
        setCurrentPage('profileChangeConsole');

        await invokeTauriCommandWrapper<void>(
            "setup_app",
            {appName, profileName: newProfileName},
            () => {
            },
            (errorMessage, rawError) => {
                console.error(`Failed to invoke setup_app for profile change on ${appName} to ${newProfileName}:`, rawError);
                setConsoleInitialMessage(prev => `${prev}\nERROR (client-side): Failed to dispatch profile change operation: ${errorMessage}`);
            }
        );
    };


    const handleBackFromProfileChangeConsole = async () => {
        setCurrentPage('list');
        resetConsoleStates();
        if (startingAppName) {
            setAppActionLoading(prev => ({...prev, [startingAppName]: false}));
        }
        setStartingAppName(null);
        setProfileChangeData(null);
        clearMessages();
        updateStatus({messageLoading: false});

        updateStatus({loading: true, info: "Refreshing app..."});
        await invokeTauriCommandWrapper<App[]>(
            "load_apps",
            undefined,
            () => {
                updateStatus({loading: false, info: "App refreshed."});
            },
            (errorMessage, rawError) => {
                console.error("Failed to reload apps after profile change attempt:", rawError);
                updateStatus({error: `Failed to reload apps: ${errorMessage}`, info: null, loading: false});
            }
        );
    };


    const navigateToSettings = () => {
        clearMessages();
        setCurrentPage('settings');
    };

    const handleBackFromSettings = () => {
        setCurrentPage('list');
    };


    const [snackbarOpen, setSnackbarOpen] = useState(false);
    const [snackbarMessage, setSnackbarMessage] = useState("");
    const [snackbarSeverity, setSnackbarSeverity] = useState<"success" | "info" | "warning" | "error">("info");

    useEffect(() => {
        if (status.info && !status.messageLoading && (currentPage === 'list' || currentPage === 'settings' || currentPage === 'changeProfile')) {
            setSnackbarMessage(status.info);
            setSnackbarSeverity("info");
            setSnackbarOpen(true);
            const timerId = window.setTimeout(() => updateStatus({info: null}), 5000);
            return () => clearTimeout(timerId);
        }
        if (status.error && !status.messageLoading && (currentPage === 'list' || currentPage === 'settings' || currentPage === 'changeProfile')) {
            setSnackbarMessage(status.error);
            setSnackbarSeverity("error");
            setSnackbarOpen(true);
            const timerId = window.setTimeout(() => updateStatus({error: null}), 8000);
            return () => clearTimeout(timerId);
        }
        if (!status.info && !status.error) {
            setSnackbarOpen(false);
        }
    }, [status.info, status.error, status.messageLoading, updateStatus, currentPage]);


    const handleSettingChange = async (name: string, value: string | number) => {
        clearMessages();
        updateStatus({messageLoading: true});

        await invokeTauriCommandWrapper<void>(
            'update_config_item',
            {name, value},
            async () => {
                const updatedConfigs = await invoke<ConfigItemFromRust[]>('get_config_payload');
                setAllConfigs(updatedConfigs);
                updateStatus({info: `${name} updated successfully.`, messageLoading: false});
            },
            (errorMessage, rawError) => {
                console.error(`Failed to update setting ${name}:`, rawError);
                updateStatus({error: `Failed to update ${name}: ${errorMessage}`, messageLoading: false});
            }
        );
    };

    const handleChangePipCacheDir = (newValue: string) => {
        handleSettingChange(PIP_CACHE_DIR_CONFIG_KEY, newValue);
    };

    const handleCheckForUpdates = async (appName: string) => {
        clearMessages();
        setAppActionLoading(prev => ({...prev, [appName]: true}));
        setCheckingUpdateForApp(appName);

        await invokeTauriCommandWrapper<void>(
            "load_apps",
            undefined,
            () => {
                updateStatus({info: t("App Refreshed.")});
            },
            (errorMessage, rawError) => {
                console.error("Failed to check for updates:", rawError);
                updateStatus({error: `Failed to check for updates: ${errorMessage}`});
            }
        );

        setAppActionLoading(prev => ({...prev, [appName]: false}));
        setCheckingUpdateForApp(null);
    };


    let pageContent;

    if (currentPage === 'installConsole' && startingAppName) {
        const consoleTitle = t('Installing App: {{appName}}', {appName: startingAppName});
        pageContent = (
            <ConsolePage
                title={consoleTitle}
                appName={startingAppName}
                initialMessage={consoleInitialMessage}
                onBack={handleBackFromInstallConsole}
                isProcessing={isInstallProcessRunning}
                onProcessComplete={() => setIsInstallProcessRunning(false)}
            />
        );
    } else if (currentPage === 'startConsole' && startingAppName) {
        pageContent = (
            <ConsolePage
                title={t('Starting App: {{appName}}', {appName: startingAppName})}
                appName={startingAppName}
                initialMessage={consoleInitialMessage}
                onBack={handleBackFromStartConsole}
                isProcessing={isStartAppProcessRunning}
                onProcessComplete={() => setIsStartAppProcessRunning(false)}
            />
        );
    } else if (currentPage === 'updateLog' && updateLogViewData) {
        pageContent = (
            <UpdateLogPage
                appName={updateLogViewData.name}
                version={updateLogViewData.version}
                actionType={updateLogViewData.actionType}
                onBack={handleBackFromUpdateLog}
                onConfirm={handleConfirmVersionChange}
            />
        );
    } else if (currentPage === 'versionChangeConsole' && versionChangeConsoleData && startingAppName) {
        const title = t('{{actionType}} App: {{appName}}', {
            actionType: t(versionChangeConsoleData.actionType),
            appName: startingAppName,
        });
        pageContent = (
            <ConsolePage
                title={title}
                appName={startingAppName}
                initialMessage={consoleInitialMessage}
                onBack={handleBackFromVersionChangeConsole}
                isProcessing={isVersionChangeProcessRunning}
                onProcessComplete={() => setIsVersionChangeProcessRunning(false)}
            />
        );
    } else if (currentPage === 'runningAppConsole' && startingAppName) {
        pageContent = (
            <ConsolePage
                title={t('Console: {{appName}}', {appName: startingAppName})}
                appName={startingAppName}
                initialMessage={consoleInitialMessage}
                onBack={handleBackFromRunningAppConsole}
                isProcessing={isRunningAppConsoleOpen}
                onProcessComplete={() => setIsRunningAppConsoleOpen(false)}
            />
        );
    } else if (currentPage === 'profileChooser' && profileChoiceApp) {
        pageContent = (
            <Container maxWidth="sm" sx={{py: 4}}>
                <Typography variant="h5" gutterBottom sx={{mb: 3}}>
                    {t('Choose Profile for {{appName}}', {appName: profileChoiceApp.name})}
                </Typography>
                {profileChoiceApp.profiles && profileChoiceApp.profiles.length > 0 ? (
                    <>
                        <FormControl fullWidth sx={{my: 2}}>
                            <InputLabel id="profile-select-label">{t('Profile')}</InputLabel>
                            <Select
                                labelId="profile-select-label"
                                value={selectedProfileForInstall}
                                label={t('Profile')}
                                onChange={(e: SelectChangeEvent<string>) => setSelectedProfileForInstall(e.target.value)}
                            >
                                {profileChoiceApp.profiles.map(profile => (
                                    <MenuItem key={profile.name} value={profile.name}>
                                        {profile.name}
                                    </MenuItem>
                                ))}
                            </Select>
                        </FormControl>
                        <Stack direction="row" spacing={2} justifyContent="flex-end" sx={{mt: 3}}>
                            <Button variant="outlined" onClick={() => {
                                setCurrentPage('list');
                                setProfileChoiceApp(null);
                            }}>
                                {t('Cancel')}
                            </Button>
                            <Button
                                variant="contained"
                                onClick={() => {
                                    if (selectedProfileForInstall) {
                                        handleInstallWithProfile(profileChoiceApp.name, selectedProfileForInstall);
                                    } else {
                                        updateStatus({error: t("Please select a profile.")});
                                    }
                                }}
                                disabled={!selectedProfileForInstall || appActionLoading[profileChoiceApp.name]}
                            >
                                {appActionLoading[profileChoiceApp.name] ? t("Starting Install...") : t("Confirm & Install")}
                            </Button>
                        </Stack>
                    </>
                ) : (
                    <>
                        <Typography sx={{my: 2}}>
                            {t("No profiles available or configured for this app. Please check the app's configuration (ok.yml).")}
                        </Typography>
                        <Button variant="outlined" onClick={() => {
                            setCurrentPage('list');
                            setProfileChoiceApp(null);
                        }}>
                            {t('Back')}
                        </Button>
                    </>
                )}
            </Container>
        );
    } else if (currentPage === 'changeProfile' && appForProfileChange) {
        pageContent = (
            <Container maxWidth="sm" sx={{py: 4}}>
                <Typography variant="h5" gutterBottom sx={{mb: 3}}>
                    {t('Change Profile for {{appName}}', {appName: appForProfileChange.name})}
                </Typography>
                <Typography variant="subtitle1" gutterBottom sx={{mb: 1}}>
                    {t('Current Profile: {{profile}}', {profile: appForProfileChange.current_profile})}
                </Typography>
                {appForProfileChange.profiles && appForProfileChange.profiles.length > 0 ? (
                    <>
                        <FormControl fullWidth sx={{my: 2}}>
                            <InputLabel id="change-profile-select-label">{t('New Profile')}</InputLabel>
                            <Select
                                labelId="change-profile-select-label"
                                value={selectedNewProfileName}
                                label={t('New Profile')}
                                onChange={(e: SelectChangeEvent<string>) => setSelectedNewProfileName(e.target.value)}
                            >
                                {appForProfileChange.profiles.map(profile => (
                                    <MenuItem key={profile.name} value={profile.name}
                                              disabled={profile.name === appForProfileChange.current_profile}>
                                        {profile.name}
                                        {profile.name === appForProfileChange.current_profile && t(" (Current)")}
                                    </MenuItem>
                                ))}
                            </Select>
                        </FormControl>
                        <Stack direction="row" spacing={2} justifyContent="flex-end" sx={{mt: 3}}>
                            <Button variant="outlined" onClick={() => {
                                setCurrentPage('list');
                                setAppForProfileChange(null);
                                setSelectedNewProfileName("");
                            }}>
                                {t('Cancel')}
                            </Button>
                            <Button
                                variant="contained"
                                onClick={() => {
                                    if (selectedNewProfileName && selectedNewProfileName !== appForProfileChange.current_profile) {
                                        handleConfirmProfileChange(appForProfileChange.name, selectedNewProfileName);
                                    } else if (selectedNewProfileName === appForProfileChange.current_profile) {
                                        updateStatus({error: t("Please select a different profile.")});
                                    } else {
                                        updateStatus({error: t("Please select a profile.")});
                                    }
                                }}
                                disabled={!selectedNewProfileName || selectedNewProfileName === appForProfileChange.current_profile || appActionLoading[appForProfileChange.name]}
                            >
                                {appActionLoading[appForProfileChange.name] ? t("Initiating...") : t("Change Profile")}
                            </Button>
                        </Stack>
                    </>
                ) : (
                    <Typography sx={{my: 2}}>
                        {t("No profiles available for this app. This view should not be reachable in this state.")}
                    </Typography>
                )}
            </Container>
        );
    } else if (currentPage === 'profileChangeConsole' && profileChangeData && startingAppName) {
        pageContent = (
            <ConsolePage
                title={t("Changing Profile: {{appName}} to '{{newProfile}}'", {
                    appName: profileChangeData.appName,
                    newProfile: profileChangeData.newProfile
                })}
                appName={startingAppName}
                initialMessage={consoleInitialMessage}
                onBack={handleBackFromProfileChangeConsole}
                isProcessing={isProfileChangeProcessRunning}
                onProcessComplete={() => setIsProfileChangeProcessRunning(false)}
            />
        );
    } else if (currentPage === 'settings') {
        if (isLoadingConfigs || !allConfigs) {
            pageContent = (
                <Container maxWidth="sm" sx={{
                    py: 4,
                    display: 'flex',
                    justifyContent: 'center',
                    alignItems: 'center',
                    height: '100vh'
                }}>
                    <CircularProgress/>
                    <Typography sx={{ml: 2}}>{t('Loading settings...')}</Typography>
                </Container>
            );
        } else {
            const pipCacheConfig = allConfigs.find(c => c.name === PIP_CACHE_DIR_CONFIG_KEY);
            const currentPipCacheDir = (pipCacheConfig?.value as string) ?? "App Install Directory";
            const pipCacheDirOptions = (pipCacheConfig?.options as string[] | undefined) ?? ["System Default", "App Install Directory"];

            pageContent = (
                <SettingsPage
                    currentTheme={themeMode}
                    onChangeTheme={setThemeMode}
                    onBack={handleBackFromSettings}
                    currentPipCacheDir={currentPipCacheDir}
                    pipCacheDirOptions={pipCacheDirOptions}
                    onChangePipCacheDir={handleChangePipCacheDir}
                />
            );
        }
    } else {
        pageContent = (
            <Container maxWidth="lg" sx={{py: 3}}>
                <Box sx={{display: 'flex', justifyContent: 'flex-end', alignItems: 'center', mb: 2}}>
                    <IconButton onClick={navigateToSettings} color="inherit" aria-label="settings"
                                title={t("Settings")}>
                        <SettingsIcon/>
                    </IconButton>
                </Box>

                {status.messageLoading && currentPage === 'list' && (
                    <Box sx={{display: 'flex', alignItems: 'center', my: 2}}>
                        <CircularProgress size={24} sx={{mr: 1}}/>
                        <Typography>{t('Processing action...')}</Typography>
                    </Box>
                )}
                <Snackbar
                    open={snackbarOpen}
                    autoHideDuration={snackbarSeverity === 'error' ? 8000 : 5000}
                    onClose={() => {
                        setSnackbarOpen(false);
                        if (snackbarSeverity === 'info') updateStatus({info: null});
                        if (snackbarSeverity === 'error') updateStatus({error: null});
                    }}
                    anchorOrigin={{vertical: 'bottom', horizontal: 'center'}}
                >
                    <Alert onClose={() => {
                        setSnackbarOpen(false);
                        if (snackbarSeverity === 'info') updateStatus({info: null});
                        if (snackbarSeverity === 'error') updateStatus({error: null});
                    }} severity={snackbarSeverity} sx={{width: '100%'}}>
                        {snackbarMessage}
                    </Alert>
                </Snackbar>

                {status.loading && apps === null && !status.messageLoading &&
                    <Box sx={{display: 'flex', justifyContent: 'center', my: 3}}><CircularProgress/><Typography
                        sx={{ml: 1}}>{t('Loading app...')}</Typography></Box>}
                {!status.loading && !status.messageLoading && !status.error && !status.info && apps && apps.length === 0 && (
                    <Typography sx={{my: 3, textAlign: 'center'}}>{t('No apps found. Add one using the form above.')}</Typography>
                )}

                {apps && apps.length > 0 && (
                    <List>
                        {apps.map((app) => {
                            const isRunning = app.running;
                            const isInstalled = app.installed;
                            const isEffectivelyInstalling = isRunning && !isInstalled;

                            const isThisAppLoadingAction = appActionLoading[app.name] || false;
                            const disableGlobalActions = currentPage !== 'list' || status.messageLoading;
                            const disableRowActions = disableGlobalActions || isThisAppLoadingAction;


                            const availableVersionsForSelect = app.available_versions.filter(v => v !== app.current_version);
                            const currentSelectedVersionForApp = selectedTargetVersions[app.name] || '';

                            let actionButtonText = t("Set Version");
                            let actionButtonIcon = <SettingsApplications/>;
                            let actionButtonColor: "primary" | "secondary" | "success" | "warning" | "error" | "info" = "primary";
                            let actionType = "Set";


                            if (currentSelectedVersionForApp && app.current_version) {
                                const comparison = compareVersions(currentSelectedVersionForApp, app.current_version);
                                if (comparison > 0) {
                                    actionType = "Update";
                                    actionButtonIcon = <Update/>;
                                    actionButtonColor = "success";
                                } else if (comparison < 0) {
                                    actionType = "Downgrade";
                                    actionButtonIcon = <ArrowDownward/>;
                                    actionButtonColor = "warning";
                                }
                                actionButtonText = t('{{action}} App', {action: t(actionType)});
                            } else if (currentSelectedVersionForApp) {
                                actionButtonText = t("Set Version");
                                actionButtonIcon = <Build/>;
                            }

                            const isVersionChangeLoading = isThisAppLoadingAction && startingAppName === app.name && isVersionChangeProcessRunning;
                            const isProfileChangeLoading = isThisAppLoadingAction && startingAppName === app.name && isProfileChangeProcessRunning;


                            return (
                                <ListItem key={app.name} disablePadding sx={{mb: 2}}>
                                    <Card variant="outlined"
                                          sx={{
                                              width: '100%',
                                              bgcolor: (isRunning || isEffectivelyInstalling) ? 'action.selected' : 'background.paper'
                                          }}>
                                        <CardContent>
                                            <Box sx={{
                                                display: 'flex',
                                                justifyContent: 'space-between',
                                                alignItems: 'center',
                                                mb: 1
                                            }}>
                                                <Typography variant="h6" component="div">
                                                    {app.name}
                                                    {isInstalled && app.current_version ? ` (${app.current_version})` : ""}
                                                    {isInstalled && app.current_profile && ` [${app.current_profile}]`}
                                                    {!isInstalled && !isEffectivelyInstalling &&
                                                        <Typography component="span" color="text.secondary"
                                                                    sx={{ml: 1}}>{t('(Not Installed)')}</Typography>}
                                                    {isEffectivelyInstalling &&
                                                        <Typography component="span" color="info.main"
                                                                    sx={{ml: 1}}>{t('(Installing...)')}</Typography>}
                                                    {isInstalled && isRunning &&
                                                        <Typography component="span" color="success.main"
                                                                    sx={{ml: 1}}>{t('(Running)')}</Typography>}
                                                </Typography>
                                            </Box>

                                            <Stack direction={{xs: 'column', sm: 'row'}} spacing={1}
                                                   sx={{mb: 1, flexWrap: 'wrap'}} alignItems="center">

                                                {isInstalled ? (
                                                    isRunning ? (
                                                        <>
                                                            <Button
                                                                variant="outlined"
                                                                color="warning"
                                                                size="small"
                                                                startIcon={isThisAppLoadingAction ?
                                                                    <CircularProgress size={16} color="inherit"/> :
                                                                    <StopCircle/>}
                                                                onClick={() => handleStopApp(app.name)}
                                                                disabled={disableRowActions}
                                                            >
                                                                {isThisAppLoadingAction ? t("Stopping...") : t("Stop App")}
                                                            </Button>
                                                            <Button
                                                                variant="outlined"
                                                                color="info"
                                                                size="small"
                                                                startIcon={<OpenInNew/>}
                                                                onClick={() => handleOpenRunningAppConsole(app.name)}
                                                                disabled={disableRowActions}
                                                            >
                                                                {t('Console')}
                                                            </Button>
                                                        </>
                                                    ) : (
                                                        <Button
                                                            variant="outlined"
                                                            color="success"
                                                            size="small"
                                                            startIcon={(isThisAppLoadingAction && startingAppName === app.name && isStartAppProcessRunning) ?
                                                                <CircularProgress size={16} color="inherit"/> :
                                                                <PlayArrow/>}
                                                            onClick={() => handleStartApp(app.name)}
                                                            disabled={disableRowActions || !app.current_version}
                                                        >
                                                            {(isThisAppLoadingAction && startingAppName === app.name && isStartAppProcessRunning) ? t("Starting...") : t("Start App")}
                                                        </Button>
                                                    )
                                                ) : isEffectivelyInstalling ? (
                                                    <>
                                                        <Button
                                                            variant="outlined"
                                                            color="info"
                                                            size="small"
                                                            startIcon={<OpenInNew/>}
                                                            onClick={() => handleOpenRunningAppConsole(app.name)}
                                                            disabled={disableRowActions}
                                                        >
                                                            {t('Console')}
                                                        </Button>
                                                    </>
                                                ) : (
                                                    <Button
                                                        variant="contained"
                                                        color="primary"
                                                        size="small"
                                                        startIcon={(isThisAppLoadingAction && startingAppName === app.name && isInstallProcessRunning) ?
                                                            <CircularProgress size={16} color="inherit"/> :
                                                            <Build/>}
                                                        onClick={() => handleInstallClick(app)}
                                                        disabled={disableRowActions || (isThisAppLoadingAction && startingAppName === app.name && isInstallProcessRunning)}
                                                    >
                                                        {(isThisAppLoadingAction && startingAppName === app.name && isInstallProcessRunning) ? t("Installing...") : t("Install")}
                                                    </Button>
                                                )}

                                                {isInstalled && !isRunning && app.profiles && app.profiles.length > 1 && (
                                                    <Button
                                                        variant="outlined"
                                                        color="secondary"
                                                        size="small"
                                                        startIcon={isProfileChangeLoading ?
                                                            <CircularProgress size={16} color="inherit"/> : <Cached/>}
                                                        onClick={() => handleNavigateToChangeProfilePage(app)}
                                                        disabled={disableRowActions}
                                                    >
                                                        {isProfileChangeLoading ? t("Changing...") : t("Change Profile")}
                                                    </Button>
                                                )}

                                                {isInstalled && (
                                                    <Button
                                                        variant="outlined"
                                                        color="error"
                                                        size="small"
                                                        startIcon={isThisAppLoadingAction && (isInstallProcessRunning || isProfileChangeProcessRunning || isVersionChangeProcessRunning || (isRunning && startingAppName === app.name)) ?
                                                            <CircularProgress size={16} color="inherit"/> : <Delete/>}
                                                        onClick={() => handleDeleteClick(app.name)}
                                                        disabled={disableGlobalActions || isRunning || (isThisAppLoadingAction && startingAppName === app.name && (isInstallProcessRunning || isProfileChangeProcessRunning || isVersionChangeProcessRunning))}
                                                    >
                                                        {isThisAppLoadingAction && startingAppName === app.name && (isRunning || isInstallProcessRunning || isProfileChangeProcessRunning || isVersionChangeProcessRunning) ? t("Deleting...") : t("Delete")}
                                                    </Button>
                                                )}
                                            </Stack>

                                            {isInstalled && !isRunning && (
                                                <Stack direction={{xs: 'column', sm: 'row'}} spacing={1}
                                                       alignItems="center"
                                                       sx={{mt: 2}}>
                                                    {availableVersionsForSelect.length > 0 ? (
                                                        <>
                                                            <FormControl size="small"
                                                                         sx={{minWidth: {xs: '100%', sm: 200}}}
                                                                         disabled={disableRowActions}>
                                                                <InputLabel
                                                                    id={`version-select-label-${app.name}`}>{t('Change version...')}</InputLabel>
                                                                <Select
                                                                    labelId={`version-select-label-${app.name}`}
                                                                    value={currentSelectedVersionForApp}
                                                                    label={t('Change version...')}
                                                                    onChange={(e: SelectChangeEvent<string>) => {
                                                                        setSelectedTargetVersions(prev => ({
                                                                            ...prev,
                                                                            [app.name]: e.target.value,
                                                                        }));
                                                                        if (!status.messageLoading) clearMessages();
                                                                    }}
                                                                >
                                                                    <MenuItem value=""
                                                                              disabled={!currentSelectedVersionForApp}>
                                                                        <em>{t('Change version...')}</em>
                                                                    </MenuItem>
                                                                    {availableVersionsForSelect.map((version) => (
                                                                        <MenuItem key={version} value={version}>
                                                                            {version}
                                                                            {app.current_version && compareVersions(version, app.current_version) > 0 && ` ${t('(Update)')}`}
                                                                            {app.current_version && compareVersions(version, app.current_version) < 0 && ` ${t('(Downgrade)')}`}
                                                                        </MenuItem>
                                                                    ))}
                                                                </Select>
                                                            </FormControl>
                                                            <Button
                                                                variant="contained"
                                                                size="small"
                                                                color={actionButtonColor}
                                                                startIcon={isVersionChangeLoading ?
                                                                    <CircularProgress size={16}
                                                                                      color="inherit"/> : actionButtonIcon}
                                                                onClick={() => handleNavigateToUpdateLogPage(app.name, currentSelectedVersionForApp, app.current_version)}
                                                                disabled={!currentSelectedVersionForApp || disableRowActions}
                                                            >
                                                                {isVersionChangeLoading ? t(`${actionType}ing...`) : actionButtonText}
                                                            </Button>
                                                        </>
                                                    ) : (
                                                        <Typography variant="caption" sx={{
                                                            flexGrow: 1,
                                                            textAlign: {xs: 'center', sm: 'left'}
                                                        }}>
                                                            {t("No other versions found.")}
                                                        </Typography>
                                                    )}
                                                    <Tooltip title={t("Check for updates")}>
                                                        <span>
                                                          <IconButton
                                                              onClick={() => handleCheckForUpdates(app.name)}
                                                              disabled={disableRowActions}
                                                              aria-label={t("Check for updates")}
                                                              size="small"
                                                          >
                                                            {isThisAppLoadingAction && checkingUpdateForApp === app.name ? (
                                                                <CircularProgress size={20}/>
                                                            ) : (
                                                                <Cached/>
                                                            )}
                                                          </IconButton>
                                                        </span>
                                                    </Tooltip>
                                                </Stack>
                                            )}
                                        </CardContent>
                                    </Card>
                                </ListItem>
                            );
                        })}
                    </List>
                )}
                <Dialog
                    open={isConfirmDeleteDialogOpen}
                    onClose={handleCancelDelete}
                    aria-labelledby="alert-dialog-title"
                    aria-describedby="alert-dialog-description"
                >
                    <DialogTitle id="alert-dialog-title">
                        {t('Confirm Deletion')}
                    </DialogTitle>
                    <DialogContent>
                        <DialogContentText id="alert-dialog-description">
                            {appToDelete && t('Are you sure you want to delete {{appName}}? This action cannot be undone.', {appName: appToDelete})}
                        </DialogContentText>
                    </DialogContent>
                    <DialogActions>
                        <Button onClick={handleCancelDelete}>{t('Cancel')}</Button>
                        <Button onClick={handleConfirmDelete} color="error" autoFocus>
                            {t('Delete')}
                        </Button>
                    </DialogActions>
                </Dialog>
            </Container>
        );
    }


    return (
        <ThemeProvider theme={muiTheme}>
            <CssBaseline/>
            <Box sx={{display: 'flex', flexDirection: 'column', minHeight: '100vh'}}>
                <Box component="main" sx={{flex: '1 1 auto'}}>
                    {pageContent}
                </Box>
                {currentPage === 'list' && (
                    <Box component="footer" sx={{py: 2, textAlign: 'center'}}>
                        <Typography variant="body2" color="text.secondary">

                            <Link href="https://github.com/ok-oldking/pyappify" target="_blank"
                                  rel="noopener noreferrer">
                                {t('appMadeWith', { name:`PyAppify ${appVersion}`} )}
                            </Link>
                        </Typography>
                    </Box>
                )}
            </Box>
        </ThemeProvider>
    );
}

export default App;