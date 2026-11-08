// src/AppCard.tsx
import type {ReactElement} from "react";
import {
    Box,
    Button,
    Card,
    CardContent,
    Chip,
    CircularProgress,
    FormControl,
    FormControlLabel,
    IconButton,
    InputLabel,
    Link,
    MenuItem,
    Select,
    Stack,
    Switch,
    Tooltip,
    Typography,
} from "@mui/material";
import {
    Build,
    Cached,
    Delete,
    KeyboardArrowRight,
    OpenInNew,
    PlayArrow,
    Settings as SettingsIcon,
    StopCircle,
} from '@mui/icons-material';
import {alpha} from '@mui/material/styles';
import {useTranslation} from 'react-i18next';
import AppIcon from "./AppIcon";
import ConsolePage, {type MessagePayload} from "./ConsolePage";
import UpdateLogPage from "./UpdateLogPage";
import {calculateVersionChangeProgress} from "./updateProgress";
import {
    compareVersions,
    getVersionActionProgressKey,
    getVersionActionType,
    getVersionChannelLabelKey,
} from "./versions";
import {
    type AppInfo,
    type InlineConsoleKind,
    type InlineUpdateLogState,
    UPDATE_METHOD_OPTIONS,
    type VersionChangeRequest,
} from "./types";

interface AppCardProps {
    app: AppInfo;
    /** Another page or a global message operation is active; disables all actions. */
    isBusy: boolean;
    /** An action for this app is in flight. */
    isActionLoading: boolean;
    inlineConsoleKind?: InlineConsoleKind;
    inlineUpdateEntry?: InlineUpdateLogState;
    logs: MessagePayload[];
    /** The inline start console's process is still running. */
    isStartProcessing: boolean;
    showDefenderButton: boolean;
    isAddingDefenderExclusion: boolean;
    isCheckingUpdates: boolean;
    selectedTargetVersion: string;
    onOpenWebsite: () => void;
    onPreferenceChange: (updateMethod: string | null, autoStart: boolean | null) => void;
    onStart: () => void;
    onStop: () => void;
    onOpenConsole: () => void;
    onInstall: () => void;
    onAddDefenderExclusion: () => void;
    onChangeProfile: () => void;
    onDelete: () => void;
    onOpenSettings: () => void;
    onCheckForUpdates: () => void;
    onSelectVersion: (version: string) => void;
    onCloseInlineConsole: () => void;
    onConfirmVersionChange: (request: VersionChangeRequest) => void;
    onOpenUpdateConsole: () => void;
    onCancelVersionChange: () => void;
}

const outlinedIconButtonSx = {border: 1, borderColor: 'divider', borderRadius: 2};

export default function AppCard({
    app,
    isBusy,
    isActionLoading,
    inlineConsoleKind,
    inlineUpdateEntry,
    logs,
    isStartProcessing,
    showDefenderButton,
    isAddingDefenderExclusion,
    isCheckingUpdates,
    selectedTargetVersion,
    onOpenWebsite,
    onPreferenceChange,
    onStart,
    onStop,
    onOpenConsole,
    onInstall,
    onAddDefenderExclusion,
    onChangeProfile,
    onDelete,
    onOpenSettings,
    onCheckForUpdates,
    onSelectVersion,
    onCloseInlineConsole,
    onConfirmVersionChange,
    onOpenUpdateConsole,
    onCancelVersionChange,
}: AppCardProps) {
    const {t} = useTranslation();

    const isEffectivelyInstalling = app.running && !app.installed;
    const persistedActionType = getVersionActionType(
        app.update_target_version ?? '',
        app.current_version,
        'Upgrade',
    );
    const disableRowActions = isBusy || isActionLoading || app.update_state !== 'idle';
    const disableUpdateControls = isBusy || isActionLoading || app.update_state === 'updating';
    const inlineUpdateAction = inlineUpdateEntry?.actionType ?? persistedActionType;
    const isUpdateProcessing = app.update_state === 'updating' || !!inlineUpdateEntry?.isConfirming;
    const otherVersions = app.available_versions.filter(v => v !== app.current_version);
    const actionIcon = (icon: ReactElement, size = 16) => isActionLoading ? <CircularProgress size={size}/> : icon;

    const statusLabel = app.running && app.installed
        ? t('(Running)')
        : isEffectivelyInstalling
            ? t('(Installing...)')
            : app.installed ? t('Installed') : t('(Not Installed)');

    const consoleButton = (
        <Button variant="outlined" color="info" size="small" startIcon={<OpenInNew/>} onClick={onOpenConsole} disabled={disableRowActions}>{t('Console')}</Button>
    );

    return (
        <Card
            variant="outlined"
            sx={{
                width: '100%',
                borderColor: app.running ? 'success.main' : 'divider',
                bgcolor: 'background.paper',
                overflow: inlineConsoleKind ? 'hidden' : 'visible',
                height: inlineConsoleKind ? '100%' : 'auto',
            }}
        >
            <CardContent sx={{
                p: {xs: 2, sm: 3},
                '&:last-child': {pb: {xs: 2, sm: 3}},
                height: inlineConsoleKind ? '100%' : 'auto',
                boxSizing: 'border-box',
                display: inlineConsoleKind ? 'flex' : 'block',
                flexDirection: inlineConsoleKind ? 'column' : undefined,
            }}>
                <Stack direction={{xs: 'column', sm: 'row'}} spacing={2} sx={{justifyContent: 'space-between', alignItems: {xs: 'stretch', sm: 'flex-start'}}}>
                    <Stack direction="row" spacing={1.5} sx={{minWidth: 0, alignItems: 'center'}}>
                        <AppIcon appName={app.name} iconPath={app.icon}/>
                        <Box sx={{minWidth: 0}}>
                            <Stack direction="row" spacing={1.5} useFlexGap sx={{alignItems: 'center', flexWrap: 'wrap'}}>
                                {app.website?.trim() ? (
                                    <Link
                                        component="button"
                                        type="button"
                                        variant="h6"
                                        underline="hover"
                                        onClick={onOpenWebsite}
                                        sx={{fontWeight: 500, maxWidth: '100%', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', textAlign: 'left'}}
                                    >
                                        {app.name}
                                    </Link>
                                ) : (
                                    <Typography variant="h6" noWrap>{app.name}</Typography>
                                )}
                                {app.installed && !app.running && (
                                    <FormControlLabel
                                        sx={{m: 0}}
                                        control={(
                                            <Switch
                                                size="small"
                                                checked={app.auto_start}
                                                disabled={disableRowActions}
                                                onChange={(event) => onPreferenceChange(null, event.target.checked)}
                                            />
                                        )}
                                        label={<Typography variant="body2" sx={{fontWeight: 600}}>{t('Auto Start')}</Typography>}
                                    />
                                )}
                            </Stack>
                            <Stack direction="row" spacing={0.75} useFlexGap sx={{mt: 0.75, flexWrap: 'wrap'}}>
                                <Chip
                                    size="small"
                                    color={app.running ? 'success' : isEffectivelyInstalling ? 'info' : 'default'}
                                    variant={app.running || isEffectivelyInstalling ? 'filled' : 'outlined'}
                                    label={statusLabel}
                                    sx={{fontWeight: 650}}
                                />
                                {app.installed && app.current_version && (
                                    <Chip size="small" variant="outlined" label={app.current_version}/>
                                )}
                                {app.installed && app.current_profile && (
                                    <Chip size="small" variant="outlined" label={app.current_profile}/>
                                )}
                                {app.update_state === 'updating' && (
                                    <Chip size="small" color="info" icon={<CircularProgress size={14}/>} label={t(getVersionActionProgressKey(persistedActionType))}/>
                                )}
                                {app.update_state === 'failed' && (
                                    <Chip size="small" color="error" label={t(`${persistedActionType} failed`)}/>
                                )}
                            </Stack>
                        </Box>
                    </Stack>
                    <Stack
                        direction="row"
                        spacing={1}
                        useFlexGap
                        sx={{flexShrink: 0, flexWrap: 'wrap', justifyContent: {xs: 'flex-start', sm: 'flex-end'}}}
                    >
                        {app.installed ? (
                            app.running ? (
                                <>
                                    <Button variant="contained" color="warning" startIcon={actionIcon(<StopCircle/>)} onClick={onStop} disabled={disableRowActions}>{t("Stop App")}</Button>
                                    {consoleButton}
                                </>
                            ) : (
                                <Button variant="contained" color="success" startIcon={actionIcon(<PlayArrow/>)} onClick={onStart} disabled={disableRowActions || !app.current_version}>{t("Start App")}</Button>
                            )
                        ) : isEffectivelyInstalling ? (
                            consoleButton
                        ) : (
                            <Button variant="contained" color="primary" startIcon={actionIcon(<Build/>)} endIcon={<KeyboardArrowRight/>} onClick={onInstall} disabled={disableRowActions}>{t("Install")}</Button>
                        )}
                        {app.show_add_defender && showDefenderButton && (
                            <Button variant="outlined" color="secondary" size="small" startIcon={isActionLoading && isAddingDefenderExclusion ? <CircularProgress size={16}/> : <Build/>} onClick={onAddDefenderExclusion} disabled={disableRowActions}>{t("Add Defender Exclusion")}</Button>
                        )}
                        {app.installed && !app.running && app.profiles?.length > 1 && (
                            <Button variant="outlined" color="secondary" size="small" startIcon={actionIcon(<Cached/>)} onClick={onChangeProfile} disabled={disableRowActions}>{t("Change Profile")}</Button>
                        )}
                        {app.installed && (
                            <Tooltip title={t('Delete')}>
                                <span>
                                    <IconButton color="error" onClick={onDelete} disabled={disableRowActions || app.running} sx={outlinedIconButtonSx}>
                                        {actionIcon(<Delete fontSize="small"/>, 18)}
                                    </IconButton>
                                </span>
                            </Tooltip>
                        )}
                        <Tooltip title={t('Settings')}>
                            <IconButton onClick={onOpenSettings} color="inherit" sx={outlinedIconButtonSx}>
                                <SettingsIcon fontSize="small"/>
                            </IconButton>
                        </Tooltip>
                    </Stack>
                </Stack>
                {app.installed && (!app.running || !!inlineConsoleKind) && (
                    <Box
                        sx={{
                            mt: 2.5,
                            p: {xs: 1.5, sm: 2},
                            borderRadius: 3,
                            bgcolor: theme => alpha(theme.palette.primary.main, theme.palette.mode === 'dark' ? 0.075 : 0.045),
                            flex: inlineConsoleKind ? '1 1 auto' : undefined,
                            minHeight: inlineConsoleKind ? 0 : undefined,
                            display: inlineConsoleKind ? 'flex' : 'block',
                            flexDirection: inlineConsoleKind ? 'column' : undefined,
                        }}
                    >
                        {!app.running && (
                            <Stack direction={{xs: 'column', sm: 'row'}} spacing={1} sx={{alignItems: {xs: 'stretch', sm: 'center'}}}>
                                <FormControl size="small" sx={{minWidth: {xs: '100%', sm: 220}}} disabled={disableUpdateControls}>
                                    <InputLabel>{t('Update Method')}</InputLabel>
                                    <Select
                                        value={app.update_method}
                                        label={t('Update Method')}
                                        onChange={(event) => onPreferenceChange(event.target.value, null)}
                                    >
                                        {UPDATE_METHOD_OPTIONS.map(option => (
                                            <MenuItem key={option} value={option}>{t(option)}</MenuItem>
                                        ))}
                                    </Select>
                                </FormControl>
                                {otherVersions.length > 0 ? (
                                    <FormControl size="small" sx={{minWidth: {xs: '100%', sm: 220}}} disabled={disableUpdateControls}>
                                        <InputLabel>{t('Change version...')}</InputLabel>
                                        <Select
                                            value={selectedTargetVersion}
                                            label={t('Change version...')}
                                            renderValue={(selected) => {
                                                const selectedVersion = String(selected);
                                                return selectedVersion ? `${selectedVersion} ${t(getVersionChannelLabelKey(selectedVersion))}` : t('Change version...');
                                            }}
                                            onChange={(e) => onSelectVersion(e.target.value)}
                                        >
                                            <MenuItem value=""><em>{t('Change version...')}</em></MenuItem>
                                            {otherVersions.map(v => (
                                                <MenuItem key={v} value={v}>
                                                    {v} {t(getVersionChannelLabelKey(v))}
                                                    {compareVersions(v, app.current_version!) > 0 ? ` ${t('(Upgrade)')}` : ` ${t('(Downgrade)')}`}
                                                </MenuItem>
                                            ))}
                                        </Select>
                                    </FormControl>
                                ) : <Typography variant="caption">{t("No other versions found.")}</Typography>}
                                <Tooltip title={t("Check for updates")}>
                                    <span>
                                        <IconButton onClick={onCheckForUpdates} disabled={disableRowActions} sx={{bgcolor: 'background.paper', ...outlinedIconButtonSx}}>
                                            {isActionLoading && isCheckingUpdates ? <CircularProgress size={20}/> : <Cached fontSize="small"/>}
                                        </IconButton>
                                    </span>
                                </Tooltip>
                            </Stack>
                        )}
                        {inlineConsoleKind ? (
                            <ConsolePage
                                inline
                                title={inlineConsoleKind === 'start'
                                    ? t('Starting App: {{appName}}', {appName: app.name})
                                    : t('{{actionType}} App: {{appName}}', {actionType: t(inlineUpdateAction), appName: app.name})}
                                appName={app.name}
                                logs={logs}
                                onBack={onCloseInlineConsole}
                                isProcessing={inlineConsoleKind === 'start' ? isStartProcessing : isUpdateProcessing}
                                progress={inlineConsoleKind === 'update' ? calculateVersionChangeProgress(logs, isUpdateProcessing) : undefined}
                                progressAction={inlineConsoleKind === 'update' ? t(inlineUpdateAction) : undefined}
                            />
                        ) : inlineUpdateEntry && (
                            <UpdateLogPage
                                appName={app.name}
                                version={inlineUpdateEntry.version}
                                actionType={inlineUpdateEntry.actionType}
                                isConfirming={inlineUpdateEntry.isConfirming}
                                completed={inlineUpdateEntry.completed}
                                failed={inlineUpdateEntry.failed}
                                website={app.website}
                                onConfirm={onConfirmVersionChange}
                                onOpenConsole={onOpenUpdateConsole}
                                onCancel={onCancelVersionChange}
                            />
                        )}
                    </Box>
                )}
            </CardContent>
        </Card>
    );
}
