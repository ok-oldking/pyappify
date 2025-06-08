// src/SettingsPage.tsx
import React from 'react';
import {
    Box,
    Button,
    Container,
    FormControl,
    InputLabel,
    MenuItem,
    Paper,
    Select,
    SelectChangeEvent,
    Typography
} from '@mui/material';

type ThemeModeSetting = 'light' | 'dark' | 'system';

interface SettingsPageProps {
    currentTheme: ThemeModeSetting;
    onChangeTheme: (theme: ThemeModeSetting) => void;

    // Props for Pip Cache Directory
    currentPipCacheDir: string;
    pipCacheDirOptions: string[];
    onChangePipCacheDir: (value: string) => void;

    // Props for Default Python Version
    currentPythonVersion: string;
    pythonVersionOptions: string[];
    onChangePythonVersion: (value: string) => void;

    // Props for Pip Index URL
    currentPipIndexUrl: string;
    pipIndexUrlOptions: (string | number)[]; // Will be string[] from backend
    pipIndexUrlDisplayMap: Record<string, string>; // For user-friendly names
    onChangePipIndexUrl: (value: string) => void;

    onBack: () => void;
}

const SettingsPage: React.FC<SettingsPageProps> = ({
                                                       currentTheme,
                                                       onChangeTheme,
                                                       currentPipCacheDir,
                                                       pipCacheDirOptions,
                                                       onChangePipCacheDir,
                                                       currentPythonVersion,
                                                       pythonVersionOptions,
                                                       onChangePythonVersion,
                                                       currentPipIndexUrl,
                                                       pipIndexUrlOptions,
                                                       pipIndexUrlDisplayMap,
                                                       onChangePipIndexUrl,
                                                       onBack
                                                   }) => {
    const handleThemeChange = (event: SelectChangeEvent<ThemeModeSetting>) => {
        onChangeTheme(event.target.value as ThemeModeSetting);
    };

    const handlePipCacheDirChange = (event: SelectChangeEvent<string>) => {
        onChangePipCacheDir(event.target.value as string);
    };

    const handlePythonVersionChange = (event: SelectChangeEvent<string>) => {
        onChangePythonVersion(event.target.value as string);
    };

    const handlePipIndexUrlChange = (event: SelectChangeEvent<string>) => {
        onChangePipIndexUrl(event.target.value as string);
    };

    return (
        <Container maxWidth="sm" sx={{py: 4}}>
            <Paper elevation={3} sx={{p: 3}}>
                <Typography variant="h4" component="h1" gutterBottom sx={{textAlign: 'center', mb: 3}}>
                    Settings
                </Typography>

                {/* Theme Setting */}
                <Box sx={{my: 2}}>
                    <FormControl fullWidth variant="outlined">
                        <InputLabel id="theme-select-label">Theme</InputLabel>
                        <Select
                            labelId="theme-select-label"
                            id="theme-select"
                            value={currentTheme}
                            label="Theme"
                            onChange={handleThemeChange}
                        >
                            <MenuItem value="system">System Default</MenuItem>
                            <MenuItem value="light">Light</MenuItem>
                            <MenuItem value="dark">Dark</MenuItem>
                        </Select>
                    </FormControl>
                </Box>

                {/* Pip Cache Directory Setting */}
                <Box sx={{my: 2}}>
                    <FormControl fullWidth variant="outlined">
                        <InputLabel id="pip-cache-dir-select-label">Pip Cache Directory</InputLabel>
                        <Select
                            labelId="pip-cache-dir-select-label"
                            id="pip-cache-dir-select"
                            value={currentPipCacheDir}
                            label="Pip Cache Directory"
                            onChange={handlePipCacheDirChange}
                        >
                            {pipCacheDirOptions.map((option) => (
                                <MenuItem key={option} value={option}>
                                    {option}
                                </MenuItem>
                            ))}
                        </Select>
                    </FormControl>
                </Box>

                {/* Default Python Version Setting */}
                <Box sx={{my: 2}}>
                    <FormControl fullWidth variant="outlined">
                        <InputLabel id="python-version-select-label">Default Python Version</InputLabel>
                        <Select
                            labelId="python-version-select-label"
                            id="python-version-select"
                            value={currentPythonVersion}
                            label="Default Python Version"
                            onChange={handlePythonVersionChange}
                        >
                            {pythonVersionOptions.length > 0 ? (
                                pythonVersionOptions.map((option) => (
                                    <MenuItem key={option} value={option}>
                                        {option}
                                    </MenuItem>
                                ))
                            ) : (
                                <MenuItem value={currentPythonVersion} disabled>
                                    {currentPythonVersion} (No other options available)
                                </MenuItem>
                            )}
                        </Select>
                    </FormControl>
                </Box>

                {/* Pip Index URL Setting */}
                <Box sx={{my: 2}}>
                    <FormControl fullWidth variant="outlined">
                        <InputLabel id="pip-index-url-select-label">Pip Index URL</InputLabel>
                        <Select
                            labelId="pip-index-url-select-label"
                            id="pip-index-url-select"
                            value={currentPipIndexUrl}
                            label="Pip Index URL"
                            onChange={handlePipIndexUrlChange}
                        >
                            {pipIndexUrlOptions.length > 0 ? (
                                pipIndexUrlOptions.map((optionValue) => (
                                    <MenuItem key={String(optionValue)} value={String(optionValue)}>
                                        {pipIndexUrlDisplayMap[String(optionValue)] || String(optionValue)}
                                    </MenuItem>
                                ))
                            ) : (
                                <MenuItem value={currentPipIndexUrl} disabled>
                                    {pipIndexUrlDisplayMap[String(currentPipIndexUrl)] || String(currentPipIndexUrl)} (No
                                    other options)
                                </MenuItem>
                            )}
                        </Select>
                    </FormControl>
                </Box>


                <Box sx={{mt: 4, display: 'flex', justifyContent: 'center'}}>
                    <Button variant="outlined" onClick={onBack}>
                        Back to App List
                    </Button>
                </Box>
            </Paper>
        </Container>
    );
};

export default SettingsPage;