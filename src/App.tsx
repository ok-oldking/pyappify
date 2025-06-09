// src/App.tsx
import {useCallback, useEffect, useMemo, useRef, useState} from "react";
import {invoke} from "@tauri-apps/api/core";
import {listen, UnlistenFn} from "@tauri-apps/api/event";
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
    FormControl,
    IconButton,
    InputLabel,
    List,
    ListItem,
    MenuItem,
    Select,
    SelectChangeEvent,
    Snackbar,
    Stack,
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
const DEFAULT_PYTHON_VERSION_CONFIG_KEY = "Default Python Version";
const PIP_INDEX_URL_CONFIG_KEY = "Pip Index URL";

const PIP_INDEX_URL_DISPLAY_OPTIONS_MAP: Record<string, string> = {
    "": "None (Use System Config File)",
    "https://pypi.org/simple/": "Pypi",
    "https://pypi.tuna.tsinghua.edu.cn/simple": "Tsinghua",
    "http://mirrors.aliyun.com/pypi/simple/": "AliYun",
    "https://mirrors.ustc.edu.cn/pypi/simple/": "ustc",
    "https://repo.huaweicloud.com/repository/pypi/simple/": "huawei",
    "https://mirrors.cloud.tencent.com/pypi/simple/": "tencent"
};

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
        const unlistenPromises: Promise<UnlistenFn>[] = [];

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

                const currentExistingSelection = selectedTargetVersionsRef.current[app.name];
                const isExistingSelectionValidAndByUser =
                    currentExistingSelection &&
                    app.available_versions.includes(currentExistingSelection) &&
                    currentExistingSelection !== app.current_version;

                if (isExistingSelectionValidAndByUser) {
                    newSelectedTargets[app.name] = currentExistingSelection;
                } else {
                    const availableForSelection = app.available_versions.filter(v => v !== app.current_version);
                    if (availableForSelection.length > 0) {
                        const sortedAvailable = [...availableForSelection].sort((a, b) => compareVersions(b, a));
                        let versionToAutoSelect: string | undefined = undefined;

                        if (app.current_version) {
                            const newestUpgrade = sortedAvailable.find(v => compareVersions(v, app.current_version!) > 0);
                            if (newestUpgrade) {
                                versionToAutoSelect = newestUpgrade;
                            }
                        } else {
                            if (sortedAvailable.length > 0) {
                                versionToAutoSelect = sortedAvailable[0];
                            }
                        }
                        if (versionToAutoSelect) {
                            newSelectedTargets[app.name] = versionToAutoSelect;
                        }
                    }
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

        updateStatus({loading: true, info: "Refreshing app list..."});
        await invokeTauriCommandWrapper<App[]>(
            "load_apps",
            undefined,
            () => {
                updateStatus({loading: false, info: "App list refreshed."});
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

        updateStatus({loading: true, info: "Refreshing app list..."});
        await invokeTauriCommandWrapper<App[]>(
            "load_apps",
            undefined,
            () => {
                updateStatus({loading: false, info: "App list refreshed."});
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

        updateStatus({loading: true, info: "Refreshing app list..."});
        await invokeTauriCommandWrapper<App[]>(
            "load_apps",
            undefined,
            () => {
                updateStatus({loading: false, info: "App list refreshed."});
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

        updateStatus({loading: true, info: "Refreshing app list..."});
        await invokeTauriCommandWrapper<App[]>(
            "load_apps",
            undefined,
            () => {
                updateStatus({loading: false, info: "App list refreshed."});
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

    const handleChangePythonVersion = (newValue: string) => {
        handleSettingChange(DEFAULT_PYTHON_VERSION_CONFIG_KEY, newValue);
    };

    const handleChangePipIndexUrl = (newValue: string) => {
        handleSettingChange(PIP_INDEX_URL_CONFIG_KEY, newValue);
    };


    let pageContent;

    if (currentPage === 'installConsole' && startingAppName) {
        const consoleTitle = `Installing App: ${startingAppName}`;
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
                title={`Starting App: ${startingAppName}`}
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
        pageContent = (
            <ConsolePage
                title={`${versionChangeConsoleData.actionType} App: ${versionChangeConsoleData.appName}`}
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
                title={`Console: ${startingAppName}`}
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
                    Choose Profile for {profileChoiceApp.name}
                </Typography>
                {profileChoiceApp.profiles && profileChoiceApp.profiles.length > 0 ? (
                    <>
                        <FormControl fullWidth sx={{my: 2}}>
                            <InputLabel id="profile-select-label">Profile</InputLabel>
                            <Select
                                labelId="profile-select-label"
                                value={selectedProfileForInstall}
                                label="Profile"
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
                                Cancel
                            </Button>
                            <Button
                                variant="contained"
                                onClick={() => {
                                    if (selectedProfileForInstall) {
                                        handleInstallWithProfile(profileChoiceApp.name, selectedProfileForInstall);
                                    } else {
                                        updateStatus({error: "Please select a profile."});
                                    }
                                }}
                                disabled={!selectedProfileForInstall || appActionLoading[profileChoiceApp.name]}
                            >
                                {appActionLoading[profileChoiceApp.name] ? "Starting Install..." : "Confirm & Install"}
                            </Button>
                        </Stack>
                    </>
                ) : (
                    <>
                        <Typography sx={{my: 2}}>
                            No profiles available or configured for this app. Please check the app's configuration
                            (ok.yml).
                        </Typography>
                        <Button variant="outlined" onClick={() => {
                            setCurrentPage('list');
                            setProfileChoiceApp(null);
                        }}>
                            Back to List
                        </Button>
                    </>
                )}
            </Container>
        );
    } else if (currentPage === 'changeProfile' && appForProfileChange) {
        pageContent = (
            <Container maxWidth="sm" sx={{py: 4}}>
                <Typography variant="h5" gutterBottom sx={{mb: 3}}>
                    Change Profile for {appForProfileChange.name}
                </Typography>
                <Typography variant="subtitle1" gutterBottom sx={{mb: 1}}>
                    Current Profile: {appForProfileChange.current_profile}
                </Typography>
                {appForProfileChange.profiles && appForProfileChange.profiles.length > 0 ? (
                    <>
                        <FormControl fullWidth sx={{my: 2}}>
                            <InputLabel id="change-profile-select-label">New Profile</InputLabel>
                            <Select
                                labelId="change-profile-select-label"
                                value={selectedNewProfileName}
                                label="New Profile"
                                onChange={(e: SelectChangeEvent<string>) => setSelectedNewProfileName(e.target.value)}
                            >
                                {appForProfileChange.profiles.map(profile => (
                                    <MenuItem key={profile.name} value={profile.name}
                                              disabled={profile.name === appForProfileChange.current_profile}>
                                        {profile.name}
                                        {profile.name === appForProfileChange.current_profile && " (Current)"}
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
                                Cancel
                            </Button>
                            <Button
                                variant="contained"
                                onClick={() => {
                                    if (selectedNewProfileName && selectedNewProfileName !== appForProfileChange.current_profile) {
                                        handleConfirmProfileChange(appForProfileChange.name, selectedNewProfileName);
                                    } else if (selectedNewProfileName === appForProfileChange.current_profile) {
                                        updateStatus({error: "Please select a different profile."});
                                    } else {
                                        updateStatus({error: "Please select a profile."});
                                    }
                                }}
                                disabled={!selectedNewProfileName || selectedNewProfileName === appForProfileChange.current_profile || appActionLoading[appForProfileChange.name]}
                            >
                                {appActionLoading[appForProfileChange.name] ? "Initiating..." : "Change Profile"}
                            </Button>
                        </Stack>
                    </>
                ) : (
                    <Typography sx={{my: 2}}>
                        No profiles available for this app. This view should not be reachable in this state.
                    </Typography>
                )}
            </Container>
        );
    } else if (currentPage === 'profileChangeConsole' && profileChangeData && startingAppName) {
        pageContent = (
            <ConsolePage
                title={`Changing Profile: ${profileChangeData.appName} to '${profileChangeData.newProfile}'`}
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
                    <Typography sx={{ml: 2}}>Loading settings...</Typography>
                </Container>
            );
        } else {
            const pipCacheConfig = allConfigs.find(c => c.name === PIP_CACHE_DIR_CONFIG_KEY);
            const pythonVersionConfig = allConfigs.find(c => c.name === DEFAULT_PYTHON_VERSION_CONFIG_KEY);
            const pipIndexUrlConfig = allConfigs.find(c => c.name === PIP_INDEX_URL_CONFIG_KEY);

            const currentPipCacheDir = (pipCacheConfig?.value as string) ?? "App Install Directory";
            const pipCacheDirOptions = (pipCacheConfig?.options as string[] | undefined) ?? ["System Default", "App Install Directory"];

            const currentPythonVersion = (pythonVersionConfig?.value as string) ?? "3.12";
            const pythonVersionOptions = (pythonVersionConfig?.options as string[] | undefined) ?? (pythonVersionConfig ? [pythonVersionConfig.default_value as string] : ["3.12"]);

            const currentPipIndexUrl = pipIndexUrlConfig ? (pipIndexUrlConfig.value as string) : "";
            const pipIndexUrlOptionsFromRust = pipIndexUrlConfig?.options ? (pipIndexUrlConfig.options as string[]) : (pipIndexUrlConfig ? [pipIndexUrlConfig.default_value as string] : [""]);


            pageContent = (
                <SettingsPage
                    currentTheme={themeMode}
                    onChangeTheme={setThemeMode}
                    onBack={handleBackFromSettings}

                    currentPipCacheDir={currentPipCacheDir}
                    pipCacheDirOptions={pipCacheDirOptions}
                    onChangePipCacheDir={handleChangePipCacheDir}

                    currentPythonVersion={currentPythonVersion}
                    pythonVersionOptions={pythonVersionOptions}
                    onChangePythonVersion={handleChangePythonVersion}

                    currentPipIndexUrl={currentPipIndexUrl}
                    pipIndexUrlOptions={pipIndexUrlOptionsFromRust}
                    pipIndexUrlDisplayMap={PIP_INDEX_URL_DISPLAY_OPTIONS_MAP}
                    onChangePipIndexUrl={handleChangePipIndexUrl}
                />
            );
        }
    } else {
        pageContent = (
            <Container maxWidth="lg" sx={{py: 3}}>
                <Box sx={{display: 'flex', justifyContent: 'flex-end', alignItems: 'center', mb: 2}}>
                    <IconButton onClick={navigateToSettings} color="inherit" aria-label="settings" title="Settings">
                        <SettingsIcon/>
                    </IconButton>
                </Box>

                {status.messageLoading && currentPage === 'list' && (
                    <Box sx={{display: 'flex', alignItems: 'center', my: 2}}>
                        <CircularProgress size={24} sx={{mr: 1}}/>
                        <Typography>Processing action...</Typography>
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
                        sx={{ml: 1}}>Loading apps list...</Typography></Box>}
                {!status.loading && !status.messageLoading && !status.error && !status.info && apps && apps.length === 0 && (
                    <Typography sx={{my: 3, textAlign: 'center'}}>No apps found. Add one using the form
                        above.</Typography>
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

                            let actionButtonText = "Select Version";
                            let actionButtonIcon = <SettingsApplications/>;
                            let actionButtonColor: "primary" | "secondary" | "success" | "warning" | "error" | "info" = "primary";


                            if (currentSelectedVersionForApp && app.current_version) {
                                const comparison = compareVersions(currentSelectedVersionForApp, app.current_version);
                                if (comparison > 0) {
                                    actionButtonText = "Update App";
                                    actionButtonIcon = <Update/>;
                                    actionButtonColor = "success";
                                } else if (comparison < 0) {
                                    actionButtonText = "Downgrade App";
                                    actionButtonIcon = <ArrowDownward/>;
                                    actionButtonColor = "warning";
                                }
                            } else if (currentSelectedVersionForApp) {
                                actionButtonText = "Set Version";
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
                                                                    sx={{ml: 1}}>(Not Installed)</Typography>}
                                                    {isEffectivelyInstalling &&
                                                        <Typography component="span" color="info.main"
                                                                    sx={{ml: 1}}>(Installing...)</Typography>}
                                                    {isInstalled && isRunning &&
                                                        <Typography component="span" color="success.main"
                                                                    sx={{ml: 1}}>(Running)</Typography>}
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
                                                                {isThisAppLoadingAction ? "Stopping..." : "Stop App"}
                                                            </Button>
                                                            <Button
                                                                variant="outlined"
                                                                color="info"
                                                                size="small"
                                                                startIcon={<OpenInNew/>}
                                                                onClick={() => handleOpenRunningAppConsole(app.name)}
                                                                disabled={disableRowActions}
                                                            >
                                                                Console
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
                                                            {(isThisAppLoadingAction && startingAppName === app.name && isStartAppProcessRunning) ? "Starting..." : "Start App"}
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
                                                            Console
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
                                                        {(isThisAppLoadingAction && startingAppName === app.name && isInstallProcessRunning) ? "Installing..." : "Install"}
                                                    </Button>
                                                )}

                                                {isInstalled && (
                                                    <Button
                                                        variant="outlined"
                                                        color="error"
                                                        size="small"
                                                        startIcon={isThisAppLoadingAction && (isInstallProcessRunning || isProfileChangeProcessRunning || isVersionChangeProcessRunning || (isRunning && startingAppName === app.name)) ?
                                                            <CircularProgress size={16} color="inherit"/> : <Delete/>}
                                                        onClick={() => handleDeleteApp(app.name)}
                                                        disabled={disableGlobalActions || isRunning || (isThisAppLoadingAction && startingAppName === app.name && (isInstallProcessRunning || isProfileChangeProcessRunning || isVersionChangeProcessRunning))}
                                                    >
                                                        {isThisAppLoadingAction && startingAppName === app.name && (isRunning || isInstallProcessRunning || isProfileChangeProcessRunning || isVersionChangeProcessRunning) ? "Deleting..." : "Delete"}
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
                                                        {isProfileChangeLoading ? "Changing..." : "Change Profile"}
                                                    </Button>
                                                )}
                                            </Stack>

                                            {isInstalled && !isRunning && app.current_version && availableVersionsForSelect.length > 0 && (
                                                <Stack direction={{xs: 'column', sm: 'row'}} spacing={1}
                                                       alignItems="center"
                                                       sx={{mt: 2}}>
                                                    <FormControl size="small" sx={{minWidth: {xs: '100%', sm: 200}}}
                                                                 disabled={disableRowActions}>
                                                        <InputLabel id={`version-select-label-${app.name}`}>
                                                            Change version...
                                                        </InputLabel>
                                                        <Select
                                                            labelId={`version-select-label-${app.name}`}
                                                            value={currentSelectedVersionForApp}
                                                            label="Change version..."
                                                            onChange={(e: SelectChangeEvent<string>) => {
                                                                setSelectedTargetVersions(prev => ({
                                                                    ...prev,
                                                                    [app.name]: e.target.value,
                                                                }));
                                                                if (!status.messageLoading) clearMessages();
                                                            }}
                                                        >
                                                            <MenuItem value="" disabled={!currentSelectedVersionForApp}>
                                                                <em>Change version...</em>
                                                            </MenuItem>
                                                            {availableVersionsForSelect.map((version) => (
                                                                <MenuItem key={version} value={version}>
                                                                    {version}
                                                                    {app.current_version && compareVersions(version, app.current_version) > 0 && ' (Update)'}
                                                                    {app.current_version && compareVersions(version, app.current_version) < 0 && ' (Downgrade)'}
                                                                </MenuItem>
                                                            ))}
                                                        </Select>
                                                    </FormControl>
                                                    <Button
                                                        variant="contained"
                                                        size="small"
                                                        color={actionButtonColor}
                                                        startIcon={isVersionChangeLoading ? <CircularProgress size={16}
                                                                                                              color="inherit"/> : actionButtonIcon}
                                                        onClick={() => handleNavigateToUpdateLogPage(app.name, currentSelectedVersionForApp, app.current_version)}
                                                        disabled={!currentSelectedVersionForApp || disableRowActions}
                                                    >
                                                        {isVersionChangeLoading ? `${actionButtonText.split(" ")[0]}ing...` : actionButtonText}
                                                    </Button>
                                                </Stack>
                                            )}


                                            {isInstalled && !isRunning && !app.current_version && (
                                                <Typography variant="caption" display="block"
                                                            sx={{mt: 1, fontStyle: 'italic'}}>
                                                    App is marked installed but has no current version. Consider
                                                    re-installing or setting a version if available.
                                                </Typography>
                                            )}
                                            {isInstalled && !isRunning && app.current_version && availableVersionsForSelect.length === 0 &&
                                                (!app.profiles || app.profiles.length <= 1) && (
                                                    <Typography variant="caption" display="block"
                                                                sx={{mt: 1, fontStyle: 'italic'}}>
                                                        No other versions or profiles available for modification.
                                                    </Typography>
                                                )}
                                            {isInstalled && !isRunning && app.current_version && availableVersionsForSelect.length === 0 &&
                                                (app.profiles && app.profiles.length > 1) && (
                                                    <Typography variant="caption" display="block"
                                                                sx={{mt: 1, fontStyle: 'italic'}}>
                                                        No other versions available. You can change the profile.
                                                    </Typography>
                                                )}
                                        </CardContent>
                                    </Card>
                                </ListItem>
                            );
                        })}
                    </List>
                )}
            </Container>
        );
    }


    return (
        <ThemeProvider theme={muiTheme}>
            <CssBaseline/>
            {pageContent}
        </ThemeProvider>
    );
}

export default App;