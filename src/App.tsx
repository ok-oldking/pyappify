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
    Snackbar,
    Stack,
    Tooltip,
    Typography
} from "@mui/material";
import {
    Build,
    Cached,
    Delete,
    OpenInNew,
    PlayArrow,
    Settings as SettingsIcon,
    StopCircle,
    Update
} from '@mui/icons-material';
import {createTheme, ThemeProvider} from '@mui/material/styles';
import CssBaseline from '@mui/material/CssBaseline';
import useMediaQuery from '@mui/material/useMediaQuery';
import {useTranslation} from 'react-i18next';
import {invokeTauriCommandWrapper} from "./utils.ts";

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
    show_add_defender: boolean;
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

export type ThemeModeSetting = 'light' | 'dark' | 'system';

function App() {
    const {t} = useTranslation();
    const [apps, setApps] = useState<App[] | null>(null);
    const [status, setStatus] = useState<StatusState>({loading: true, error: null, info: null, messageLoading: false});
    const [appActionLoading, setAppActionLoading] = useState<Record<string, boolean>>({});
    const [selectedTargetVersions, setSelectedTargetVersions] = useState<Record<string, string>>({});
    const selectedTargetVersionsRef = useRef(selectedTargetVersions);
    const [currentPage, setCurrentPage] = useState<Page>('list');
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
    const [profileChoiceApp, setProfileChoiceApp] = useState<App | null>(null);
    const [selectedProfileForInstall, setSelectedProfileForInstall] = useState<string>("");
    const [appForProfileChange, setAppForProfileChange] = useState<App | null>(null);
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

    useEffect(() => {
        selectedTargetVersionsRef.current = selectedTargetVersions;
    }, [selectedTargetVersions]);

    useEffect(() => {
        localStorage.setItem('appThemeMode', themeMode);
    }, [themeMode]);

    const prefersDarkMode = useMediaQuery('(prefers-color-scheme: dark)');
    const muiTheme = useMemo(() => {
        const mode: 'light' | 'dark' = themeMode === 'system' ? (prefersDarkMode ? 'dark' : 'light') : themeMode;
        return createTheme({ palette: { mode } });
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

        await invokeTauriCommandWrapper<void>("start_app", {appName}, () => {},
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

        await invokeTauriCommandWrapper<void>("setup_app", {appName, profileName}, () => {},
            (errorMessage, rawError) => {
                console.error(`Failed to invoke setup_app for ${appName} with profile ${profileName}:`, rawError);
                setConsoleInitialMessage(prev => `${prev}\nERROR (client-side): Failed to dispatch install operation: ${errorMessage}`);
            }
        );
    };

    const handleInstallClick = (app: App) => {
        if (app.profiles && app.profiles.length > 1) {
            setProfileChoiceApp(app);
            const initialProfile = app.profiles.some(p => p.name === app.current_profile)
                ? app.current_profile
                : app.profiles[0]?.name || "default";
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
            const newApps = event.payload;
            setApps(newApps);
            const newSelectedTargets: Record<string, string> = {};
            newApps.forEach(app => {
                if (!app.installed || app.running) {
                    if (selectedTargetVersionsRef.current[app.name]) newSelectedTargets[app.name] = '';
                    return;
                }
                const sortedVersions = [...app.available_versions].sort((a, b) => compareVersions(b, a));
                const latestVersion = sortedVersions[0];
                const currentSelection = selectedTargetVersionsRef.current[app.name];
                if (app.current_version && latestVersion && compareVersions(latestVersion, app.current_version) > 0) {
                    newSelectedTargets[app.name] = latestVersion;
                } else if (currentSelection && app.available_versions.includes(currentSelection) && currentSelection !== app.current_version) {
                    newSelectedTargets[app.name] = currentSelection;
                } else {
                    newSelectedTargets[app.name] = '';
                }
            });
            setSelectedTargetVersions(prev => ({...prev, ...newSelectedTargets}));
            updateStatus({loading: false});
        }));

        unlistenPromises.push(listen<App>("choose_app_profile", (event) => {
            const app = event.payload;
            setProfileChoiceApp(app);
            const initialProfile = app.profiles?.some(p => p.name === app.current_profile)
                ? app.current_profile
                : app.profiles?.[0]?.name || "default";
            setSelectedProfileForInstall(initialProfile);
            setCurrentPage('profileChooser');
        }));

        (async () => {
            await invokeTauriCommandWrapper<App[]>("load_apps", undefined, () => {},
                (errorMessage, rawError) => {
                    console.error("Failed to initially load apps:", rawError);
                    updateStatus({error: `Failed to load apps: ${errorMessage}`, loading: false});
                }
            );
        })();

        return () => {
            Promise.all(unlistenPromises).then(unlisteners => unlisteners.forEach(fn => fn()));
        };
    }, [updateStatus]);

    const handleDeleteApp = async (appName: string) => {
        clearMessages();
        updateStatus({messageLoading: true});
        setAppActionLoading(prev => ({...prev, [appName]: true}));

        await invokeTauriCommandWrapper<void>("delete_app", {appName}, () => {},
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
        setAppActionLoading(prev => ({...prev, [appName]: true}));
        await invokeTauriCommandWrapper<void>("stop_app", {appName}, () => {},
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
            updateStatus({error: `Please select a version for ${appName}.`});
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
        if (updateLogViewData?.name) {
            setAppActionLoading(prev => ({...prev, [updateLogViewData.name!]: false}));
        }
    };

    const handleConfirmVersionChange = async (params: { appName: string, version: string, actionType: string }) => {
        clearMessages();
        setAppActionLoading(prev => ({...prev, [params.appName]: true}));
        setVersionChangeConsoleData(params);
        setStartingAppName(params.appName);
        setConsoleInitialMessage(`Initiating ${params.actionType} for '${params.appName}' to version '${params.version}'...`);
        setIsVersionChangeProcessRunning(true);
        setCurrentPage('versionChangeConsole');

        const app = apps?.find(a => a.name === params.appName);
        const requirementsFile = app?.profiles?.find(p => p.name === app.current_profile)?.requirements || "requirements.txt";

        await invokeTauriCommandWrapper<void>("update_to_version", {appName: params.appName, version: params.version, requirements: requirementsFile}, () => {},
            (errorMessage, rawError) => {
                console.error(`Failed to invoke ${params.actionType.toLowerCase()}:`, rawError);
                setConsoleInitialMessage(prev => `${prev}\nERROR (client-side): Failed to dispatch operation: ${errorMessage}`);
            }
        );
    };

    const handleOpenRunningAppConsole = (appName: string) => {
        clearMessages();
        setStartingAppName(appName);
        const app = apps?.find(a => a.name === appName);
        const consoleTitle = (app?.running && !app?.installed) ? `Installation console for: ${appName}` : `Console for running app: ${appName}`;
        setConsoleInitialMessage(consoleTitle);
        setIsRunningAppConsoleOpen(true);
        setCurrentPage('runningAppConsole');
    };

    const resetConsoleStates = () => {
        setConsoleInitialMessage(undefined);
        setIsInstallProcessRunning(false);
        setIsStartAppProcessRunning(false);
        setIsVersionChangeProcessRunning(false);
        setIsRunningAppConsoleOpen(false);
        setIsProfileChangeProcessRunning(false);
    }

    const handleBackFromConsole = async () => {
        setCurrentPage('list');
        resetConsoleStates();
        clearMessages();
        updateStatus({messageLoading: false});
        if (startingAppName) setAppActionLoading(prev => ({...prev, [startingAppName]: false}));
        setStartingAppName(null);
        setVersionChangeConsoleData(null);
        setProfileChangeData(null);

        updateStatus({loading: true, info: t("Refreshing app...")});
        await invokeTauriCommandWrapper<App[]>("load_apps", undefined,
            () => {
                updateStatus({loading: false, info: t("App Refreshed.")});
            },
            (errorMessage, rawError) => {
                console.error("Failed to reload apps:", rawError);
                updateStatus({error: `Failed to reload apps: ${errorMessage}`, loading: false});
            }
        );
    };

    const handleNavigateToChangeProfilePage = (appToChange: App) => {
        clearMessages();
        setAppForProfileChange(appToChange);
        const initialProfile = appToChange.profiles?.some(p => p.name === appToChange.current_profile)
            ? appToChange.current_profile
            : appToChange.profiles?.[0]?.name || "";
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

        await invokeTauriCommandWrapper<void>("setup_app", {appName, profileName: newProfileName}, () => {},
            (errorMessage, rawError) => {
                console.error(`Failed to invoke setup_app for profile change:`, rawError);
                setConsoleInitialMessage(prev => `${prev}\nERROR (client-side): Failed to dispatch operation: ${errorMessage}`);
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
        setAppActionLoading(prev => ({...prev, [appName]: true}));
        setCheckingUpdateForApp(appName);
        await invokeTauriCommandWrapper<void>("load_apps", undefined,
            () => updateStatus({info: t("App Refreshed.")}),
            (errorMessage, rawError) => {
                console.error("Failed to check for updates:", rawError);
                updateStatus({error: `Failed to check for updates: ${errorMessage}`});
            }
        );
        setAppActionLoading(prev => ({...prev, [appName]: false}));
        setCheckingUpdateForApp(null);
    };

    const handleAddDefenderExclusion = async (appName: string) => {
        clearMessages();
        setAppActionLoading(prev => ({...prev, [appName]: true}));
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
        setAppActionLoading(prev => ({...prev, [appName]: false}));
    };

    let pageContent;

    if (currentPage === 'installConsole' && startingAppName) {
        pageContent = <ConsolePage title={t('Installing App: {{appName}}', {appName: startingAppName})} appName={startingAppName} initialMessage={consoleInitialMessage} onBack={handleBackFromConsole} isProcessing={isInstallProcessRunning} onProcessComplete={() => setIsInstallProcessRunning(false)} />;
    } else if (currentPage === 'startConsole' && startingAppName) {
        pageContent = <ConsolePage title={t('Starting App: {{appName}}', {appName: startingAppName})} appName={startingAppName} initialMessage={consoleInitialMessage} onBack={handleBackFromConsole} isProcessing={isStartAppProcessRunning} onProcessComplete={() => setIsStartAppProcessRunning(false)} />;
    } else if (currentPage === 'updateLog' && updateLogViewData) {
        pageContent = <UpdateLogPage appName={updateLogViewData.name} version={updateLogViewData.version} actionType={updateLogViewData.actionType} onBack={handleBackFromUpdateLog} onConfirm={handleConfirmVersionChange} />;
    } else if (currentPage === 'versionChangeConsole' && versionChangeConsoleData && startingAppName) {
        const title = t('{{actionType}} App: {{appName}}', { actionType: t(versionChangeConsoleData.actionType), appName: startingAppName });
        pageContent = <ConsolePage title={title} appName={startingAppName} initialMessage={consoleInitialMessage} onBack={handleBackFromConsole} isProcessing={isVersionChangeProcessRunning} onProcessComplete={() => setIsVersionChangeProcessRunning(false)} />;
    } else if (currentPage === 'runningAppConsole' && startingAppName) {
        pageContent = <ConsolePage title={t('Console: {{appName}}', {appName: startingAppName})} appName={startingAppName} initialMessage={consoleInitialMessage} onBack={handleBackFromConsole} isProcessing={isRunningAppConsoleOpen} onProcessComplete={() => setIsRunningAppConsoleOpen(false)} />;
    } else if (currentPage === 'profileChangeConsole' && profileChangeData && startingAppName) {
        pageContent = <ConsolePage title={t("Changing Profile: {{appName}} to '{{newProfile}}'", { appName: profileChangeData.appName, newProfile: profileChangeData.newProfile })} appName={startingAppName} initialMessage={consoleInitialMessage} onBack={handleBackFromConsole} isProcessing={isProfileChangeProcessRunning} onProcessComplete={() => setIsProfileChangeProcessRunning(false)} />;
    } else if (currentPage === 'settings') {
        pageContent = <SettingsPage currentTheme={themeMode} onChangeTheme={setThemeMode} onBack={() => setCurrentPage('list')} updateStatus={updateStatus} clearMessages={clearMessages} />;
    } else if (currentPage === 'profileChooser' && profileChoiceApp) {
        pageContent = (
            <Container maxWidth="sm" sx={{py: 4}}>
                <Typography variant="h5" gutterBottom>{t('Choose Profile for {{appName}}', {appName: profileChoiceApp.name})}</Typography>
                {profileChoiceApp.profiles?.length > 0 ? (
                    <>
                        <FormControl fullWidth sx={{my: 2}}>
                            <InputLabel id="profile-select-label">{t('Profile')}</InputLabel>
                            <Select labelId="profile-select-label" value={selectedProfileForInstall} label={t('Profile')} onChange={(e) => setSelectedProfileForInstall(e.target.value)}>
                                {profileChoiceApp.profiles.map(p => <MenuItem key={p.name} value={p.name}>{p.name}</MenuItem>)}
                            </Select>
                        </FormControl>
                        <Stack direction="row" spacing={2} justifyContent="flex-end" sx={{mt: 3}}>
                            <Button variant="outlined" onClick={() => setCurrentPage('list')}>{t('Cancel')}</Button>
                            <Button variant="contained" onClick={() => handleInstallWithProfile(profileChoiceApp.name, selectedProfileForInstall)} disabled={!selectedProfileForInstall || appActionLoading[profileChoiceApp.name]}>
                                {appActionLoading[profileChoiceApp.name] ? t("Starting Install...") : t("Confirm & Install")}
                            </Button>
                        </Stack>
                    </>
                ) : (
                    <>
                        <Typography sx={{my: 2}}>{t("No profiles available.")}</Typography>
                        <Button variant="outlined" onClick={() => setCurrentPage('list')}>{t('Back')}</Button>
                    </>
                )}
            </Container>
        );
    } else if (currentPage === 'changeProfile' && appForProfileChange) {
        pageContent = (
            <Container maxWidth="sm" sx={{py: 4}}>
                <Typography variant="h5" gutterBottom>{t('Change Profile for {{appName}}', {appName: appForProfileChange.name})}</Typography>
                <Typography variant="subtitle1" gutterBottom>{t('Current Profile: {{profile}}', {profile: appForProfileChange.current_profile})}</Typography>
                {appForProfileChange.profiles?.length > 0 ? (
                    <>
                        <FormControl fullWidth sx={{my: 2}}>
                            <InputLabel id="change-profile-select-label">{t('New Profile')}</InputLabel>
                            <Select labelId="change-profile-select-label" value={selectedNewProfileName} label={t('New Profile')} onChange={(e) => setSelectedNewProfileName(e.target.value)}>
                                {appForProfileChange.profiles.map(p => <MenuItem key={p.name} value={p.name} disabled={p.name === appForProfileChange.current_profile}>{p.name}{p.name === appForProfileChange.current_profile && t(" (Current)")}</MenuItem>)}
                            </Select>
                        </FormControl>
                        <Stack direction="row" spacing={2} justifyContent="flex-end" sx={{mt: 3}}>
                            <Button variant="outlined" onClick={() => setCurrentPage('list')}>{t('Cancel')}</Button>
                            <Button variant="contained" onClick={() => handleConfirmProfileChange(appForProfileChange.name, selectedNewProfileName)} disabled={!selectedNewProfileName || selectedNewProfileName === appForProfileChange.current_profile || appActionLoading[appForProfileChange.name]}>
                                {appActionLoading[appForProfileChange.name] ? t("Initiating...") : t("Change Profile")}
                            </Button>
                        </Stack>
                    </>
                ) : <Typography sx={{my: 2}}>{t("No profiles available.")}</Typography>}
            </Container>
        );
    } else {
        pageContent = (
            <Container maxWidth="lg" sx={{py: 3}}>
                <Box sx={{display: 'flex', justifyContent: 'flex-end', alignItems: 'center', mb: 2}}>
                    <IconButton onClick={() => setCurrentPage('settings')} color="inherit" title={t("Settings")}><SettingsIcon/></IconButton>
                </Box>
                {status.messageLoading && <Box sx={{display: 'flex', alignItems: 'center', my: 2}}><CircularProgress size={24} sx={{mr: 1}}/><Typography>{t('Processing action...')}</Typography></Box>}
                <Snackbar open={snackbarOpen} autoHideDuration={6000} onClose={() => setSnackbarOpen(false)} anchorOrigin={{vertical: 'bottom', horizontal: 'center'}}>
                    <Alert onClose={() => setSnackbarOpen(false)} severity={snackbarSeverity} sx={{width: '100%'}}>{snackbarMessage}</Alert>
                </Snackbar>
                {status.loading && !apps && <Box sx={{display: 'flex', justifyContent: 'center', my: 3}}><CircularProgress/><Typography sx={{ml: 1}}>{t('Loading app...')}</Typography></Box>}
                {!status.loading && apps?.length === 0 && <Typography sx={{my: 3, textAlign: 'center'}}>{t('No apps found.')}</Typography>}
                {apps && apps.length > 0 && (
                    <List>
                        {apps.map((app) => {
                            const isEffectivelyInstalling = app.running && !app.installed;
                            const isThisAppLoading = appActionLoading[app.name] || false;
                            const disableRowActions = currentPage !== 'list' || status.messageLoading || isThisAppLoading;
                            return (
                                <ListItem key={app.name} disablePadding sx={{mb: 2}}>
                                    <Card variant="outlined" sx={{width: '100%', bgcolor: (app.running) ? 'action.selected' : 'background.paper'}}>
                                        <CardContent>
                                            <Typography variant="h6" component="div">
                                                {app.name}
                                                {app.installed && app.current_version && ` (${app.current_version})`}
                                                {app.installed && app.current_profile && ` [${app.current_profile}]`}
                                                {!app.installed && !isEffectivelyInstalling && <Typography component="span" color="text.secondary" sx={{ml: 1}}>{t('(Not Installed)')}</Typography>}
                                                {isEffectivelyInstalling && <Typography component="span" color="info.main" sx={{ml: 1}}>{t('(Installing...)')}</Typography>}
                                                {app.installed && app.running && <Typography component="span" color="success.main" sx={{ml: 1}}>{t('(Running)')}</Typography>}
                                            </Typography>
                                            <Stack direction={{xs: 'column', sm: 'row'}} spacing={1} sx={{my: 1, flexWrap: 'wrap'}} alignItems="center">
                                                {app.installed ? (
                                                    app.running ? (
                                                        <>
                                                            <Button variant="outlined" color="warning" size="small" startIcon={isThisAppLoading ? <CircularProgress size={16}/> : <StopCircle/>} onClick={() => handleStopApp(app.name)} disabled={disableRowActions}>{t("Stop App")}</Button>
                                                            <Button variant="outlined" color="info" size="small" startIcon={<OpenInNew/>} onClick={() => handleOpenRunningAppConsole(app.name)} disabled={disableRowActions}>{t('Console')}</Button>
                                                        </>
                                                    ) : (
                                                        <Button variant="outlined" color="success" size="small" startIcon={isThisAppLoading ? <CircularProgress size={16}/> : <PlayArrow/>} onClick={() => handleStartApp(app.name)} disabled={disableRowActions || !app.current_version}>{t("Start App")}</Button>
                                                    )
                                                ) : isEffectivelyInstalling ? (
                                                    <Button variant="outlined" color="info" size="small" startIcon={<OpenInNew/>} onClick={() => handleOpenRunningAppConsole(app.name)} disabled={disableRowActions}>{t('Console')}</Button>
                                                ) : (
                                                    <Button variant="contained" color="primary" size="small" startIcon={isThisAppLoading ? <CircularProgress size={16}/> : <Build/>} onClick={() => handleInstallClick(app)} disabled={disableRowActions}>{t("Install")}</Button>
                                                )}
                                                {app.show_add_defender && !hiddenDefenderButtons.has(app.name) && <Button variant="outlined" color="secondary" size="small" startIcon={isThisAppLoading && addingDefenderExclusionForApp === app.name ? <CircularProgress size={16}/> : <Build/>} onClick={() => handleAddDefenderExclusion(app.name)} disabled={disableRowActions}>{t("Add Defender Exclusion")}</Button>}
                                                {app.installed && !app.running && app.profiles?.length > 1 && <Button variant="outlined" color="secondary" size="small" startIcon={isThisAppLoading ? <CircularProgress size={16}/> : <Cached/>} onClick={() => handleNavigateToChangeProfilePage(app)} disabled={disableRowActions}>{t("Change Profile")}</Button>}
                                                {app.installed && <Button variant="outlined" color="error" size="small" startIcon={isThisAppLoading ? <CircularProgress size={16}/> : <Delete/>} onClick={() => handleDeleteClick(app.name)} disabled={disableRowActions || app.running}>{t("Delete")}</Button>}
                                            </Stack>
                                            {app.installed && !app.running && (
                                                <Stack direction={{xs: 'column', sm: 'row'}} spacing={1} alignItems="center" sx={{mt: 2}}>
                                                    {app.available_versions.filter(v => v !== app.current_version).length > 0 ? (
                                                        <>
                                                            <FormControl size="small" sx={{minWidth: {xs: '100%', sm: 200}}} disabled={disableRowActions}>
                                                                <InputLabel>{t('Change version...')}</InputLabel>
                                                                <Select value={selectedTargetVersions[app.name] || ''} label={t('Change version...')} onChange={(e) => setSelectedTargetVersions(p => ({...p, [app.name]: e.target.value}))}>
                                                                    <MenuItem value=""><em>{t('Change version...')}</em></MenuItem>
                                                                    {app.available_versions.filter(v => v !== app.current_version).map(v => <MenuItem key={v} value={v}>{v}{compareVersions(v, app.current_version!) > 0 ? ` ${t('(Update)')}` : ` ${t('(Downgrade)')}`}</MenuItem>)}
                                                                </Select>
                                                            </FormControl>
                                                            <Button variant="contained" size="small" color={selectedTargetVersions[app.name] && compareVersions(selectedTargetVersions[app.name], app.current_version!) > 0 ? "success" : "warning"} startIcon={isThisAppLoading ? <CircularProgress size={16}/> : <Update/>} onClick={() => handleNavigateToUpdateLogPage(app.name, selectedTargetVersions[app.name], app.current_version)} disabled={!selectedTargetVersions[app.name] || disableRowActions}>{t("Change Version")}</Button>
                                                        </>
                                                    ) : <Typography variant="caption">{t("No other versions found.")}</Typography>}
                                                    <Tooltip title={t("Check for updates")}><span><IconButton onClick={() => handleCheckForUpdates(app.name)} disabled={disableRowActions} size="small">{isThisAppLoading && checkingUpdateForApp === app.name ? <CircularProgress size={20}/> : <Cached/>}</IconButton></span></Tooltip>
                                                </Stack>
                                            )}
                                        </CardContent>
                                    </Card>
                                </ListItem>
                            );
                        })}
                    </List>
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
                {currentPage === 'list' && (
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
