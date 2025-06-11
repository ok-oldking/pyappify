import React, {useEffect, useState} from 'react';
import {invoke} from "@tauri-apps/api/core";
import {Alert, Box, Button, CircularProgress, Container, Paper, Stack, Typography} from "@mui/material";
import ArrowBackIcon from '@mui/icons-material/ArrowBack';
import {useTranslation} from 'react-i18next';

interface UpdateLogPageProps {
    appName: string;
    version: string;
    actionType: string; // "Update", "Downgrade", "Set"
    onBack: () => void;
    onConfirm: (params: { appName: string, version: string, actionType: string }) => void;
}

const UpdateLogPage: React.FC<UpdateLogPageProps> = ({
                                                         appName,
                                                         version,
                                                         actionType,
                                                         onBack,
                                                         onConfirm,
                                                     }) => {
    const {t} = useTranslation();
    const [isConfirmingAction, setIsConfirmingAction] = useState(false);
    const [notes, setNotes] = useState<string | null>(null);
    const [notesLoading, setNotesLoading] = useState(true);
    const [notesError, setNotesError] = useState<string | null>(null);

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

    const handleConfirm = async () => {
        setIsConfirmingAction(true);
        onConfirm({appName, version, actionType});
    };

    const confirmButtonText = isConfirmingAction
        ? t(`${actionType}ing...`)
        : t('Confirm {{actionType}}', {
            actionType: actionType,
            actionTypeInChinese: t(actionType)
        });

    const pageTitle = t('{{actionType}} Notes for {{appName}} (Version: {{version}})', {
        actionType: t(actionType),
        appName: appName,
        version: version
    });

    return (
        <Container maxWidth="md" sx={{py: 3}}>
            <Button
                variant="outlined"
                startIcon={<ArrowBackIcon/>}
                onClick={onBack}
                sx={{mb: 3, alignSelf: 'flex-start'}}
                disabled={isConfirmingAction}
            >
                {t('Back to App List')}
            </Button>

            <Typography variant="h5" component="h2" gutterBottom>
                {pageTitle}
            </Typography>

            {notesLoading && (
                <Box sx={{display: 'flex', justifyContent: 'center', alignItems: 'center', my: 3}}>
                    <CircularProgress sx={{mr: 1}}/>
                    <Typography>{t('Loading notes...')}</Typography>
                </Box>
            )}
            {notesError && (
                <Alert severity="error" sx={{my: 2}}>
                    {notesError}
                </Alert>
            )}

            {notes && !notesLoading && !notesError && (
                <Paper elevation={1} sx={{
                    p: 2,
                    mt: 2,
                    whiteSpace: 'pre-wrap',
                    fontFamily: 'monospace',
                    maxHeight: '60vh',
                    overflowY: 'auto',
                }}>
                    {notes}
                </Paper>
            )}

            {!notesLoading && (
                <Stack direction="row" spacing={2} justifyContent="flex-end" sx={{mt: 3}}>
                    <Button
                        variant="outlined"
                        onClick={onBack}
                        disabled={isConfirmingAction}
                    >
                        {t('Cancel')}
                    </Button>
                    <Button
                        variant="contained"
                        onClick={handleConfirm}
                        disabled={notesLoading || !!notesError || !notes || isConfirmingAction}
                        startIcon={isConfirmingAction ? <CircularProgress size={20} color="inherit"/> : null}
                    >
                        {confirmButtonText}
                    </Button>
                </Stack>
            )}
        </Container>
    );
};

export default UpdateLogPage;