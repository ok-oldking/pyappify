import React, {useEffect, useRef} from 'react';
import {openUrl} from '@tauri-apps/plugin-opener';
import {Alert, Box, Button, CircularProgress, Container, Link, Paper, Typography} from "@mui/material";
import {useTranslation} from 'react-i18next';

export type MessagePayload = {
    message: string;
    app_name: string;
    update?: boolean;
    finished?: boolean;
    error?: boolean;
};

interface ConsolePageProps {
    title: string;
    appName: string;
    logs: MessagePayload[];
    onBack: () => void;
    isProcessing: boolean;
}

const renderMessageWithClickableLinks = (message: string) => {
    const urlRegex = /(https?:\/\/[^\s]+)/g;
    const parts = message.split(urlRegex);

    return parts.map((part, index) => {
        if (part.match(urlRegex)) {
            return (
                <Link
                    key={index}
                    href={part}
                    onClick={async (e: React.MouseEvent<HTMLAnchorElement>) => {
                        e.preventDefault();
                        try {
                            await openUrl(part);
                        } catch (error) {
                            console.error("Failed to open URL:", error);
                        }
                    }}
                    target="_blank"
                    rel="noopener noreferrer"
                    sx={{color: 'primary.main', textDecoration: 'underline', cursor: 'pointer'}}
                >
                    {part}
                </Link>
            );
        }
        return part;
    });
};


const ConsolePage: React.FC<ConsolePageProps> = ({
                                                     title,
                                                     appName,
                                                     logs,
                                                     onBack,
                                                     isProcessing,
                                                 }) => {
    const {t} = useTranslation();
    const consoleBodyRef = useRef<null | HTMLDivElement>(null);
    const lastFinishedLog = [...logs].reverse().find(log => log.finished);
    const internalIsProcessing = isProcessing && !lastFinishedLog;
    const processCompletedWithError = lastFinishedLog ? !!lastFinishedLog.error : null;

    useEffect(() => {
        if (consoleBodyRef.current) {
            consoleBodyRef.current.scrollTop = consoleBodyRef.current.scrollHeight;
        }
    }, [logs]);

    const displayMessage = internalIsProcessing
        ? t("Process in progress...")
        : t("Process finished.{{errorText}} Review logs and click Done.", {errorText: processCompletedWithError ? t(" There were errors.") : ""});

    const alertSeverity = internalIsProcessing
        ? "info"
        : (processCompletedWithError ? "error" : "success");


    return (
        <Container maxWidth="lg" sx={{
            py: 3,
            display: 'flex',
            flexDirection: 'column',
            height: 'calc(100vh - 48px)'
        }}>
            <Box sx={{mb: 2}}>
                <Typography variant="h5" component="h2" gutterBottom>
                    {title}
                </Typography>
                <Alert severity={alertSeverity} icon={internalIsProcessing ? <CircularProgress size={20}/> : undefined}>
                    {displayMessage}
                </Alert>
            </Box>

            <Paper
                elevation={3}
                sx={{
                    flexGrow: 1,
                    overflow: 'auto',
                    p: 2,
                    fontFamily: 'monospace',
                    whiteSpace: 'pre-wrap',
                    wordBreak: 'break-all',
                    backgroundColor: (theme) => theme.palette.mode === 'dark' ? theme.palette.grey[900] : theme.palette.grey[100],
                    color: 'text.primary'
                }}
                ref={consoleBodyRef}
            >
                {logs.filter(logPayload => !logPayload.finished || !!logPayload.message).map((logPayload, index) => (
                    <Typography
                        key={index}
                        component="div"
                        sx={{
                            color: logPayload.error ? 'error.main' : 'text.primary',
                            mb: 0.5,
                            fontFamily: 'monospace',
                        }}
                    >
                        {renderMessageWithClickableLinks(logPayload.message)}
                    </Typography>
                ))}
                {logs.length === 0 && !internalIsProcessing &&
                    <Typography>{t('No logs received yet for {{appName}}.', {appName})}</Typography>}
            </Paper>

            <Box sx={{pt: 2, display: 'flex', justifyContent: 'flex-end'}}>
                <Button variant="contained" onClick={onBack}>
                    {internalIsProcessing ? t("Back (Process Running)") : t("Done")}
                </Button>
            </Box>
        </Container>
    );
};

export default ConsolePage;
