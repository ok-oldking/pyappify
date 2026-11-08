// src/ProfileSelectPage.tsx
import {Button, Container, FormControl, InputLabel, MenuItem, Select, Stack, Typography} from "@mui/material";
import {useTranslation} from "react-i18next";
import type {AppInfo} from "./types";

interface ProfileSelectPageProps {
    app: AppInfo;
    title: string;
    subtitle?: string;
    selectLabel: string;
    selectedProfile: string;
    onSelectProfile: (profileName: string) => void;
    /** When set, this profile is shown as "(Current)" and cannot be chosen. */
    currentProfile?: string;
    confirmLabel: string;
    busyLabel: string;
    isBusy: boolean;
    onConfirm: () => void;
    onCancel: () => void;
    /** Label for the button shown when the app has no profiles; omitted hides it. */
    emptyBackLabel?: string;
}

/** Shared page for choosing an install profile and for changing profiles. */
export default function ProfileSelectPage({
    app,
    title,
    subtitle,
    selectLabel,
    selectedProfile,
    onSelectProfile,
    currentProfile,
    confirmLabel,
    busyLabel,
    isBusy,
    onConfirm,
    onCancel,
    emptyBackLabel,
}: ProfileSelectPageProps) {
    const {t} = useTranslation();
    const labelId = `profile-select-label-${currentProfile === undefined ? 'install' : 'change'}`;
    const canConfirm = !!selectedProfile && selectedProfile !== currentProfile && !isBusy;

    return (
        <Container maxWidth="sm" sx={{py: 4}}>
            <Typography variant="h5" gutterBottom>{title}</Typography>
            {subtitle && <Typography variant="subtitle1" gutterBottom>{subtitle}</Typography>}
            {app.profiles?.length > 0 ? (
                <>
                    <FormControl fullWidth sx={{my: 2}}>
                        <InputLabel id={labelId}>{selectLabel}</InputLabel>
                        <Select labelId={labelId} value={selectedProfile} label={selectLabel} onChange={(e) => onSelectProfile(e.target.value)}>
                            {app.profiles.map(p => (
                                <MenuItem key={p.name} value={p.name} disabled={p.name === currentProfile}>
                                    {p.name}{p.name === currentProfile && t(" (Current)")}
                                </MenuItem>
                            ))}
                        </Select>
                    </FormControl>
                    <Stack direction="row" spacing={2} sx={{mt: 3, justifyContent: 'flex-end'}}>
                        <Button variant="outlined" onClick={onCancel}>{t('Cancel')}</Button>
                        <Button variant="contained" onClick={onConfirm} disabled={!canConfirm}>
                            {isBusy ? busyLabel : confirmLabel}
                        </Button>
                    </Stack>
                </>
            ) : (
                <>
                    <Typography sx={{my: 2}}>{t("No profiles available.")}</Typography>
                    {emptyBackLabel && <Button variant="outlined" onClick={onCancel}>{emptyBackLabel}</Button>}
                </>
            )}
        </Container>
    );
}
