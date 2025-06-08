import React, {useCallback, useEffect, useRef, useState} from 'react';
import {listen, UnlistenFn} from "@tauri-apps/api/event";
import {openUrl} from '@tauri-apps/plugin-opener';
import {Alert, Box, Button, CircularProgress, Container, Link, Paper, Typography} from "@mui/material";

const MAX_LOGS = 500;

type MessagePayload = {
    message: string;
    app_name: string;
    update?: boolean;
    finished?: boolean;
    error?: boolean;
};

interface ConsolePageProps {
    title: string;
    appName: string;
    initialMessage?: string;
    initialMessageIsError?: boolean;
    onBack: () => void;
    isProcessing: boolean;
    onProcessComplete: () => void;
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
                            // Optionally show an error to the user
                        }
                    }}
                    target="_blank" // Good practice for external links
                    rel="noopener noreferrer" // Security for target="_blank"
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
                                                     initialMessage,
                                                     initialMessageIsError = false,
                                                     onBack,
                                                     isProcessing: initialIsProcessing,
                                                     onProcessComplete
                                                 }) => {
    const [logs, setLogs] = useState<MessagePayload[]>([]);
    const consoleBodyRef = useRef<null | HTMLDivElement>(null);
    const [internalIsProcessing, setInternalIsProcessing] = useState(initialIsProcessing);
    const [processCompletedWithError, setProcessCompletedWithError] = useState<boolean | null>(null);

    useEffect(() => {
        setInternalIsProcessing(initialIsProcessing);
        if (!initialIsProcessing) {
            // If initially not processing, assume success unless a finish event says otherwise
            // This handles cases where the component mounts in an already completed state.
            if (processCompletedWithError === null) { // Only set if not already determined by a previous finish event
                // setProcessCompletedWithError(false); // Or, leave it null and let the logic below handle it.
                // The current logic for displayMessage/alertSeverity handles null as success.
            }
        }
    }, [initialIsProcessing]);

    const addLog = useCallback((logEntry: MessagePayload) => {
        setLogs(prevLogs => {
            let newLogsArray;
            if (logEntry.update && prevLogs.length > 0 && prevLogs[prevLogs.length - 1].app_name === logEntry.app_name) {
                newLogsArray = [...prevLogs];
                newLogsArray[newLogsArray.length - 1] = logEntry;
            } else {
                newLogsArray = [...prevLogs, logEntry];
            }

            if (newLogsArray.length > MAX_LOGS) {
                return newLogsArray.slice(newLogsArray.length - MAX_LOGS);
            }
            return newLogsArray;
        });
    }, []);

    useEffect(() => {
        if (initialMessage) {
            initialMessage.split('\n').forEach(msgPart => {
                if (msgPart.trim() !== "") {
                    addLog({
                        message: msgPart,
                        app_name: appName,
                        error: initialMessageIsError,
                    });
                }
            });
        }
    }, [initialMessage, initialMessageIsError, appName, addLog]);

    useEffect(() => {
        const unlistenPromises: Promise<UnlistenFn>[] = [];

        unlistenPromises.push(listen<MessagePayload>("app-log", (event) => {
            const eventData = event.payload;

            if (eventData.app_name === appName) {
                addLog(eventData);

                if (eventData.finished) {
                    setInternalIsProcessing(false);
                    setProcessCompletedWithError(!!eventData.error); // Set based on the finish event's error status
                    onProcessComplete();
                }
            }
        }));

        return () => {
            Promise.all(unlistenPromises).then(unlisteners => {
                unlisteners.forEach(unlistenFn => {
                    if (typeof unlistenFn === 'function') {
                        unlistenFn();
                    }
                });
            }).catch(err => console.error("Error during unlisten cleanup in ConsolePage:", err));
        };
    }, [appName, addLog, onProcessComplete]);

    useEffect(() => {
        if (consoleBodyRef.current) {
            consoleBodyRef.current.scrollTop = consoleBodyRef.current.scrollHeight;
        }
    }, [logs]);

    const displayMessage = internalIsProcessing
        ? "Process in progress..."
        : `Process finished.${processCompletedWithError ? " There were errors." : ""} Review logs and click Done.`;

    const alertSeverity = internalIsProcessing
        ? "info"
        : (processCompletedWithError ? "error" : "success");


    return (
        <Container maxWidth="lg" sx={{
            py: 3,
            display: 'flex',
            flexDirection: 'column',
            height: 'calc(100vh - 48px)' /* Adjust if you have a global app bar */
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
                {logs.map((logPayload, index) => (
                    <Typography
                        key={index}
                        component="div" // Using div to allow block display for each log line
                        sx={{
                            color: logPayload.error ? 'error.main' : 'text.primary',
                            mb: 0.5, // Small margin between log lines
                            fontFamily: 'monospace', // Ensure monospace for pre-like formatting
                        }}
                    >
                        {renderMessageWithClickableLinks(logPayload.message)}
                    </Typography>
                ))}
                {logs.length === 0 && !internalIsProcessing &&
                    <Typography>No logs received yet for {appName}.</Typography>}
            </Paper>

            <Box sx={{pt: 2, display: 'flex', justifyContent: 'flex-end'}}>
                <Button variant="contained" onClick={onBack}>
                    {internalIsProcessing ? "Back (Process Running)" : "Done"}
                </Button>
            </Box>
        </Container>
    );
};

export default ConsolePage;