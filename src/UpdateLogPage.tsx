// src/UpdateLogPage.tsx
import React, {useEffect, useRef, useState} from 'react';
import {invoke} from "@tauri-apps/api/core";
import {Alert, Box, Button, CircularProgress, Paper, Stack, Typography} from "@mui/material";
import {useTranslation} from 'react-i18next';
import CheckCircleOutlineIcon from '@mui/icons-material/CheckCircleOutline';
import ErrorOutlineIcon from '@mui/icons-material/ErrorOutline';

interface UpdateLogPanelProps {
    appName: string;
    version: string;
    actionType: string;
    isConfirming?: boolean;
    completed?: boolean;
    failed?: boolean;
    onConfirm: (params: { appName: string, version: string, actionType: string }) => void;
    onCancel: () => void;
}

const sendOsNotification = (title: string, body: string) => {
    invoke('send_notification_cmd', {title, body}).catch(err =>
        console.warn('Failed to send OS notification:', err)
    );
};

const UpdateLogPage: React.FC<UpdateLogPanelProps> = ({
                                                          appName,
                                                          version,
                                                          actionType,
                                                          isConfirming = false,
                                                          completed = false,
                                                          failed = false,
                                                          onConfirm,
                                                          onCancel,
                                                      }) => {
    const {t} = useTranslation();
    const [notes, setNotes] = useState<string | null>(null);
    const [notesLoading, setNotesLoading] = useState(true);
    const [notesError, setNotesError] = useState<string | null>(null);

    // Track previous completed/failed to fire notification exactly once on transition
    const prevCompletedRef = useRef(completed);
    const prevFailedRef = useRef(failed);

    useEffect(() => {
        const fetchNotes = async () => {
            setNotesLoading(true);
            setNotes(null);
            setNotesError(null);
            try {
                const fetchedNotes = await invoke<string[]>("get_update_notes", {appName, version});
                setNotes(fetchedNotes.join("\n"));
            } catch (err) {
                console.error(`Failed to get notes for ${appName} version ${version}:`, err);
                const errorMessage = err instanceof Error ? err.message : String(err);
                setNotesError(t('Failed to load notes: {{error}}', {error: errorMessage}));
            } finally {
                setNotesLoading(false);
            }
        };

        if (appName && version) {
            fetchNotes();
        }
    }, [appName, version, t]);

    // Fire OS notification when completed or failed state changes
    useEffect(() => {
        const wasCompleted = prevCompletedRef.current;
        const wasFailed = prevFailedRef.current;
        prevCompletedRef.current = completed;
        prevFailedRef.current = failed;

        if (!wasCompleted && completed) {
            const title = `${t(`${actionType} success`)}: ${appName}`;
            const body = notes ? `${version}\n${notes}` : version;
            sendOsNotification(title, body);
        } else if (!wasFailed && failed) {
            const title = `${t(`${actionType} failed`)}: ${appName}`;
            const body = notes ? `${version}\n${notes}` : version;
            sendOsNotification(title, body);
        }
    }, [completed, failed, actionType, appName, version, notes, t]);

    const handleConfirm = () => {
        // Send notification when user manually triggers update/downgrade
        const title = `${t(actionType)}: ${appName}`;
        const body = notes ? `${version}\n${notes}` : version;
        sendOsNotification(title, body);
        onConfirm({appName, version, actionType});
    };

    const confirmButtonText = isConfirming
        ? t(`${actionType}ing...`)
        : t('Confirm {{actionType}}', {actionType: t(actionType)});

    let pageTitle: string;
    let borderColor: string;
    let titleColor: string;
    let titleIcon: React.ReactNode = null;

    if (completed) {
        pageTitle = `${t(`${actionType} success`)}: ${version}`;
        borderColor = 'success.main';
        titleColor = 'success.main';
        titleIcon = <CheckCircleOutlineIcon fontSize="small" color="success"/>;
    } else if (failed) {
        pageTitle = `${t(`${actionType} failed`)}: ${version}`;
        borderColor = 'error.main';
        titleColor = 'error.main';
        titleIcon = <ErrorOutlineIcon fontSize="small" color="error"/>;
    } else if (isConfirming) {
        pageTitle = `${t(`${actionType}ing...`)}: ${version}`;
        borderColor = 'info.main';
        titleColor = 'info.main';
        titleIcon = <CircularProgress size={14}/>;
    } else {
        pageTitle = `${t(actionType)}: ${version}`;
        borderColor = 'divider';
        titleColor = 'text.primary';
    }

    return (
        <Box sx={{mt: 2, border: 1, borderColor, borderRadius: 1, p: 2}}>
            <Stack direction="row" alignItems="center" spacing={0.5} sx={{mb: 0.5}}>
                {titleIcon}
                <Typography variant="subtitle1" fontWeight="bold" color={titleColor}>
                    {pageTitle}
                </Typography>
            </Stack>

            {notesLoading && (
                <Box sx={{display: 'flex', alignItems: 'center', my: 1}}>
                    <CircularProgress size={18} sx={{mr: 1}}/>
                    <Typography variant="body2">{t('Loading notes...')}</Typography>
                </Box>
            )}
            {notesError && (
                <Alert severity="error" sx={{my: 1}}>
                    {notesError}
                </Alert>
            )}

            {notes && !notesLoading && !notesError && (
                <Paper elevation={0} variant="outlined" sx={{
                    p: 1.5,
                    mt: 1,
                    whiteSpace: 'pre-wrap',
                    fontFamily: 'monospace',
                    fontSize: '0.8rem',
                    maxHeight: '200px',
                    overflowY: 'auto',
                    bgcolor: 'action.hover',
                }}>
                    {notes}
                </Paper>
            )}

            {/* Buttons: hidden while confirming/completed; only Cancel shown on failure */}
            {!notesLoading && !completed && !isConfirming && (
                <Stack direction="row" spacing={1} justifyContent="flex-end" sx={{mt: 2}}>
                    <Button
                        variant="outlined"
                        size="small"
                        onClick={onCancel}
                    >
                        {t('Cancel')}
                    </Button>
                    {!failed && (
                        <Button
                            variant="contained"
                            size="small"
                            color={actionType === 'Update' ? 'success' : 'warning'}
                            onClick={handleConfirm}
                            disabled={notesLoading || !!notesError}
                        >
                            {confirmButtonText}
                        </Button>
                    )}
                </Stack>
            )}
        </Box>
    );
};

export default UpdateLogPage;