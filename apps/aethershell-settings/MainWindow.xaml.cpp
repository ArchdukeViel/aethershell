#include "pch.h"
#include "MainWindow.xaml.h"
#if __has_include("MainWindow.g.cpp")
#include "MainWindow.g.cpp"
#endif
#include "IpcClient.h"
#include <winrt/Windows.Data.Json.h>

using namespace winrt;
using namespace Microsoft::UI::Xaml;
using namespace Microsoft::UI::Xaml::Controls;
using namespace Microsoft::UI::Xaml::Media;
using namespace Windows::Data::Json;

namespace winrt::AetherShell::Settings::implementation
{
    MainWindow::MainWindow()
    {
        InitializeComponent();
        this->Title(L"AetherShell Settings & Diagnostics");
        this->Closed({ this, &MainWindow::OnWindowClosed });

        // Query canonical configuration over secure IPC on launch
        LoadConfiguration();

        // Default navigation to General page on launch
        NavigateToPage(L"general");
    }

    void MainWindow::NavView_SelectionChanged(
        NavigationView const&,
        NavigationViewSelectionChangedEventArgs const& args)
    {
        if (auto selectedItem = args.SelectedItemContainer())
        {
            auto tag = unbox_value<hstring>(selectedItem.Tag());
            NavigateToPage(tag);
        }
    }

    void MainWindow::NavigateToPage(hstring const& tag)
    {
        if (!ContentFrame())
        {
            return;
        }

        if (tag == L"general")
        {
            ContentFrame().Content(CreateGeneralView());
        }
        else if (tag == L"taskbar")
        {
            ContentFrame().Content(CreateTaskbarView());
        }
        else if (tag == L"desktop")
        {
            ContentFrame().Content(CreateDesktopView());
        }
        else if (tag == L"widgets")
        {
            ContentFrame().Content(CreateWidgetsView());
        }
        else if (tag == L"audio")
        {
            ContentFrame().Content(CreateAudioView());
        }
        else if (tag == L"scripts")
        {
            ContentFrame().Content(CreateScriptsView());
        }
    }

    void MainWindow::OnWindowClosed(
        Windows::Foundation::IInspectable const&,
        WindowEventArgs const&)
    {
        // Enforce Gate G11: Process exits cleanly on close to eliminate resident RAM footprint (0 MB resident)
        if (auto app = Application::Current())
        {
            app.Exit();
        }
        else
        {
            PostQuitMessage(0);
        }
    }

    void MainWindow::LoadConfiguration()
    {
        m_isLoading = true;
        auto res = ::AetherShell::Settings::SettingsIpcClient::SendCommand("get_config");
        if (res.success && !res.payload.empty())
        {
            try
            {
                auto root = JsonObject::Parse(winrt::to_hstring(res.payload));
                if (root.HasKey(L"config"))
                {
                    m_config = root.GetNamedObject(L"config");
                }
            }
            catch (...)
            {
                m_config = nullptr;
            }
        }

        if (!m_config)
        {
            EnsureDefaultConfig();
        }
        m_isLoading = false;
    }

    bool MainWindow::SaveConfiguration()
    {
        if (m_isLoading || !m_config)
        {
            return false;
        }

        try
        {
            JsonObject payload;
            payload.SetNamedValue(L"config", m_config);
            std::string jsonStr = winrt::to_string(payload.Stringify());

            auto res = ::AetherShell::Settings::SettingsIpcClient::SendCommand("set_config", jsonStr);
            return res.success && res.applied;
        }
        catch (...)
        {
            return false;
        }
    }

    void MainWindow::EnsureDefaultConfig()
    {
        m_config = JsonObject();
        m_config.SetNamedValue(L"schema_version", JsonValue::CreateStringValue(L"1.0.0"));
        m_config.SetNamedValue(L"profile", JsonValue::CreateStringValue(L"signature_minimal"));

        // General
        JsonObject general;
        general.SetNamedValue(L"start_with_windows", JsonValue::CreateBooleanValue(true));
        general.SetNamedValue(L"language", JsonValue::CreateStringValue(L"system"));
        general.SetNamedValue(L"check_updates_manually", JsonValue::CreateBooleanValue(true));
        general.SetNamedValue(L"skin_library_path", JsonValue::CreateStringValue(L""));
        general.SetNamedValue(L"show_tray_icon", JsonValue::CreateBooleanValue(true));
        general.SetNamedValue(L"safe_mode", JsonValue::CreateBooleanValue(false));
        m_config.SetNamedValue(L"general", general);

        // Taskbar
        JsonObject taskbar;
        taskbar.SetNamedValue(L"enabled", JsonValue::CreateBooleanValue(true));
        taskbar.SetNamedValue(L"engine", JsonValue::CreateStringValue(L"external_appearance"));
        taskbar.SetNamedValue(L"on_unsupported", JsonValue::CreateStringValue(L"native_noop"));
        taskbar.SetNamedValue(L"conflict_policy", JsonValue::CreateStringValue(L"yield_and_notify"));
        taskbar.SetNamedValue(L"per_monitor_rules", JsonValue::CreateBooleanValue(true));
        taskbar.SetNamedValue(L"allow_experimental_explorer_bridge", JsonValue::CreateBooleanValue(false));
        taskbar.SetNamedValue(L"app_rules", JsonArray());

        JsonObject states;
        JsonObject stDesktop;
        stDesktop.SetNamedValue(L"effect", JsonValue::CreateStringValue(L"clear"));
        stDesktop.SetNamedValue(L"opacity", JsonValue::CreateNumberValue(0.0));
        states.SetNamedValue(L"desktop", stDesktop);

        JsonObject stMaximized;
        stMaximized.SetNamedValue(L"effect", JsonValue::CreateStringValue(L"tint"));
        stMaximized.SetNamedValue(L"color", JsonValue::CreateStringValue(L"#141925"));
        stMaximized.SetNamedValue(L"opacity", JsonValue::CreateNumberValue(0.85));
        states.SetNamedValue(L"maximized", stMaximized);

        JsonObject stFlyout;
        stFlyout.SetNamedValue(L"effect", JsonValue::CreateStringValue(L"solid"));
        stFlyout.SetNamedValue(L"color", JsonValue::CreateStringValue(L"#171D2A"));
        stFlyout.SetNamedValue(L"opacity", JsonValue::CreateNumberValue(1.0));
        states.SetNamedValue(L"flyout", stFlyout);

        JsonObject stBattery;
        stBattery.SetNamedValue(L"effect", JsonValue::CreateStringValue(L"solid"));
        stBattery.SetNamedValue(L"color", JsonValue::CreateStringValue(L"#171D2A"));
        stBattery.SetNamedValue(L"opacity", JsonValue::CreateNumberValue(1.0));
        states.SetNamedValue(L"battery_saver", stBattery);

        taskbar.SetNamedValue(L"states", states);
        m_config.SetNamedValue(L"taskbar", taskbar);

        // Desktop
        JsonObject desktop;
        desktop.SetNamedValue(L"placement", JsonValue::CreateStringValue(L"between_wallpaper_and_icons"));
        desktop.SetNamedValue(L"allow_always_on_top", JsonValue::CreateBooleanValue(false));
        desktop.SetNamedValue(L"allow_behind_wallpaper", JsonValue::CreateBooleanValue(false));
        desktop.SetNamedValue(L"locked", JsonValue::CreateBooleanValue(true));
        desktop.SetNamedValue(L"interaction_mode", JsonValue::CreateStringValue(L"temporary_only"));
        desktop.SetNamedValue(L"interactive_timeout_seconds", JsonValue::CreateNumberValue(8));
        desktop.SetNamedValue(L"snap_grid_dip", JsonValue::CreateNumberValue(8.0));
        desktop.SetNamedValue(L"autohide_taskbar_policy", JsonValue::CreateStringValue(L"reserve_expanded_safe_area"));
        m_config.SetNamedValue(L"desktop", desktop);

        // Audio
        JsonObject audio;
        audio.SetNamedValue(L"visualizer_enabled", JsonValue::CreateBooleanValue(true));
        audio.SetNamedValue(L"capture_source", JsonValue::CreateStringValue(L"default_playback_endpoint"));
        audio.SetNamedValue(L"store_audio", JsonValue::CreateBooleanValue(false));
        audio.SetNamedValue(L"render_fps", JsonValue::CreateNumberValue(30));
        audio.SetNamedValue(L"spectrum_bands", JsonValue::CreateNumberValue(64));
        audio.SetNamedValue(L"auto_hide_delay_seconds", JsonValue::CreateNumberValue(10));
        m_config.SetNamedValue(L"audio", audio);

        // Telemetry
        JsonObject telemetry;
        telemetry.SetNamedValue(L"sample_interval_ms", JsonValue::CreateNumberValue(1000));
        telemetry.SetNamedValue(L"on_battery_sample_interval_ms", JsonValue::CreateNumberValue(5000));
        telemetry.SetNamedValue(L"allow_optional_vendor_providers", JsonValue::CreateBooleanValue(true));
        telemetry.SetNamedValue(L"avoid_waking_discrete_gpu", JsonValue::CreateBooleanValue(true));
        m_config.SetNamedValue(L"telemetry", telemetry);

        // Power
        JsonObject power;
        power.SetNamedValue(L"profile", JsonValue::CreateStringValue(L"balanced"));
        power.SetNamedValue(L"pause_on_session_lock", JsonValue::CreateBooleanValue(true));
        power.SetNamedValue(L"pause_on_monitor_off", JsonValue::CreateBooleanValue(true));
        m_config.SetNamedValue(L"power", power);

        // Gaming
        JsonObject gaming;
        gaming.SetNamedValue(L"enabled", JsonValue::CreateBooleanValue(true));
        gaming.SetNamedValue(L"mode", JsonValue::CreateStringValue(L"per_monitor_conservative"));
        gaming.SetNamedValue(L"force_global_dormancy", JsonValue::CreateBooleanValue(false));
        m_config.SetNamedValue(L"gaming", gaming);

        // Scripting
        JsonObject scripting;
        scripting.SetNamedValue(L"enabled", JsonValue::CreateBooleanValue(true));
        scripting.SetNamedValue(L"runtime", JsonValue::CreateStringValue(L"lua54_separate_worker"));
        scripting.SetNamedValue(L"allow_standard_libraries", JsonValue::CreateBooleanValue(true));
        scripting.SetNamedValue(L"worker_executable", JsonValue::CreateStringValue(L"aethershell-script-host.exe"));
        scripting.SetNamedValue(L"restart_after_crash_limit", JsonValue::CreateNumberValue(2));
        scripting.SetNamedValue(L"require_explicit_trust_for_imported_scripts", JsonValue::CreateBooleanValue(true));
        m_config.SetNamedValue(L"scripting", scripting);

        // Privacy
        JsonObject privacy;
        privacy.SetNamedValue(L"offline_first", JsonValue::CreateBooleanValue(true));
        privacy.SetNamedValue(L"automatic_telemetry_upload", JsonValue::CreateBooleanValue(false));
        privacy.SetNamedValue(L"crash_upload", JsonValue::CreateBooleanValue(false));
        privacy.SetNamedValue(L"network_widgets_enabled_by_default", JsonValue::CreateBooleanValue(false));
        m_config.SetNamedValue(L"privacy", privacy);

        // Updates
        JsonObject updates;
        updates.SetNamedValue(L"channel", JsonValue::CreateStringValue(L"stable"));
        updates.SetNamedValue(L"auto_apply_without_consent", JsonValue::CreateBooleanValue(false));
        updates.SetNamedValue(L"retain_rollback_release", JsonValue::CreateBooleanValue(true));
        m_config.SetNamedValue(L"updates", updates);
    }

    JsonObject MainWindow::GetOrCreateSection(hstring const& sectionName)
    {
        if (!m_config)
        {
            EnsureDefaultConfig();
        }
        if (m_config.HasKey(sectionName))
        {
            try
            {
                return m_config.GetNamedObject(sectionName);
            }
            catch (...) {}
        }
        JsonObject sec;
        m_config.SetNamedValue(sectionName, sec);
        return sec;
    }

    void MainWindow::SetSection(hstring const& sectionName, JsonObject const& section)
    {
        if (!m_config)
        {
            EnsureDefaultConfig();
        }
        m_config.SetNamedValue(sectionName, section);
    }

    UIElement MainWindow::CreateGeneralView()
    {
        StackPanel panel;
        panel.Spacing(16);

        TextBlock title;
        title.Text(L"General Configuration");
        title.FontSize(24);
        panel.Children().Append(title);

        // Query Daemon status over IPC
        auto ipcStatus = ::AetherShell::Settings::SettingsIpcClient::SendCommand("get_status");
        TextBlock statusText;
        if (ipcStatus.success)
        {
            statusText.Text(L"Core Engine: Active & Connected (Named Pipe IPC)");
        }
        else
        {
            statusText.Text(L"Core Engine: Disconnected (Operating in local offline mode)");
        }
        panel.Children().Append(statusText);

        auto general = GetOrCreateSection(L"general");

        ToggleSwitch autostartSwitch;
        autostartSwitch.Header(box_value(L"Start AetherShell automatically with Windows"));
        autostartSwitch.IsOn(general.GetNamedBoolean(L"start_with_windows", true));
        autostartSwitch.Toggled([this](IInspectable const& sender, RoutedEventArgs const&) {
            if (m_isLoading) return;
            auto s = sender.as<ToggleSwitch>();
            auto general = GetOrCreateSection(L"general");
            general.SetNamedValue(L"start_with_windows", JsonValue::CreateBooleanValue(s.IsOn()));
            SetSection(L"general", general);
            SaveConfiguration();
        });
        panel.Children().Append(autostartSwitch);

        ToggleSwitch safeModeSwitch;
        safeModeSwitch.Header(box_value(L"Safe Mode (PRD Section 3.4 / Q21:A)"));
        safeModeSwitch.IsOn(general.GetNamedBoolean(L"safe_mode", false));
        safeModeSwitch.Toggled([this](IInspectable const& sender, RoutedEventArgs const&) {
            if (m_isLoading) return;
            auto s = sender.as<ToggleSwitch>();
            auto general = GetOrCreateSection(L"general");
            general.SetNamedValue(L"safe_mode", JsonValue::CreateBooleanValue(s.IsOn()));
            SetSection(L"general", general);
            SaveConfiguration();
        });
        panel.Children().Append(safeModeSwitch);

        ComboBox langBox;
        langBox.Header(box_value(L"Display Language"));
        ComboBoxItem itemSys, itemEn, itemJa, itemDe;
        itemSys.Content(box_value(L"System Default"));
        itemEn.Content(box_value(L"English (United States)"));
        itemJa.Content(box_value(L"Japanese (日本語)"));
        itemDe.Content(box_value(L"German (Deutsch)"));
        langBox.Items().Append(itemSys);
        langBox.Items().Append(itemEn);
        langBox.Items().Append(itemJa);
        langBox.Items().Append(itemDe);

        hstring curLang = general.GetNamedString(L"language", L"system");
        if (curLang == L"en-US") langBox.SelectedIndex(1);
        else if (curLang == L"ja-JP") langBox.SelectedIndex(2);
        else if (curLang == L"de-DE") langBox.SelectedIndex(3);
        else langBox.SelectedIndex(0);

        langBox.SelectionChanged([this](IInspectable const& sender, SelectionChangedEventArgs const&) {
            if (m_isLoading) return;
            auto cb = sender.as<ComboBox>();
            hstring chosen = L"system";
            if (cb.SelectedIndex() == 1) chosen = L"en-US";
            else if (cb.SelectedIndex() == 2) chosen = L"ja-JP";
            else if (cb.SelectedIndex() == 3) chosen = L"de-DE";

            auto general = GetOrCreateSection(L"general");
            general.SetNamedValue(L"language", JsonValue::CreateStringValue(chosen));
            SetSection(L"general", general);
            SaveConfiguration();
        });
        panel.Children().Append(langBox);

        ToggleSwitch updateSwitch;
        updateSwitch.Header(box_value(L"Check Updates Manually (Offline First)"));
        updateSwitch.IsOn(general.GetNamedBoolean(L"check_updates_manually", true));
        updateSwitch.Toggled([this](IInspectable const& sender, RoutedEventArgs const&) {
            if (m_isLoading) return;
            auto s = sender.as<ToggleSwitch>();
            auto general = GetOrCreateSection(L"general");
            general.SetNamedValue(L"check_updates_manually", JsonValue::CreateBooleanValue(s.IsOn()));
            SetSection(L"general", general);
            SaveConfiguration();
        });
        panel.Children().Append(updateSwitch);

        Button shutdownBtn;
        shutdownBtn.Content(box_value(L"Terminate Core Daemon"));
        shutdownBtn.Click([](auto const&, auto const&) {
            ::AetherShell::Settings::SettingsIpcClient::SendCommand("shutdown");
        });
        panel.Children().Append(shutdownBtn);

        return panel;
    }

    UIElement MainWindow::CreateTaskbarView()
    {
        StackPanel panel;
        panel.Spacing(16);

        TextBlock title;
        title.Text(L"Taskbar & State Management");
        title.FontSize(24);
        panel.Children().Append(title);

        auto taskbar = GetOrCreateSection(L"taskbar");

        ToggleSwitch enabledSwitch;
        enabledSwitch.Header(box_value(L"Enable Taskbar Customization Engine"));
        enabledSwitch.IsOn(taskbar.GetNamedBoolean(L"enabled", true));
        enabledSwitch.Toggled([this](IInspectable const& sender, RoutedEventArgs const&) {
            if (m_isLoading) return;
            auto s = sender.as<ToggleSwitch>();
            auto taskbar = GetOrCreateSection(L"taskbar");
            taskbar.SetNamedValue(L"enabled", JsonValue::CreateBooleanValue(s.IsOn()));
            SetSection(L"taskbar", taskbar);
            SaveConfiguration();
        });
        panel.Children().Append(enabledSwitch);

        ToggleSwitch multiMonSwitch;
        multiMonSwitch.Header(box_value(L"Render Taskbars on Secondary Displays (Per-Monitor Rules)"));
        multiMonSwitch.IsOn(taskbar.GetNamedBoolean(L"per_monitor_rules", true));
        multiMonSwitch.Toggled([this](IInspectable const& sender, RoutedEventArgs const&) {
            if (m_isLoading) return;
            auto s = sender.as<ToggleSwitch>();
            auto taskbar = GetOrCreateSection(L"taskbar");
            taskbar.SetNamedValue(L"per_monitor_rules", JsonValue::CreateBooleanValue(s.IsOn()));
            SetSection(L"taskbar", taskbar);
            SaveConfiguration();
        });
        panel.Children().Append(multiMonSwitch);

        ComboBox conflictBox;
        conflictBox.Header(box_value(L"Competing Customizer Conflict Policy (PRD Q21:A)"));
        ComboBoxItem polYield, polNative, polOverride;
        polYield.Content(box_value(L"Yield & Notify (Recommended)"));
        polNative.Content(box_value(L"Native No-Op Fallback"));
        polOverride.Content(box_value(L"Override Always"));
        conflictBox.Items().Append(polYield);
        conflictBox.Items().Append(polNative);
        conflictBox.Items().Append(polOverride);

        hstring curPol = taskbar.GetNamedString(L"conflict_policy", L"yield_and_notify");
        if (curPol == L"native_noop") conflictBox.SelectedIndex(1);
        else if (curPol == L"override_always") conflictBox.SelectedIndex(2);
        else conflictBox.SelectedIndex(0);

        conflictBox.SelectionChanged([this](IInspectable const& sender, SelectionChangedEventArgs const&) {
            if (m_isLoading) return;
            auto cb = sender.as<ComboBox>();
            hstring chosen = L"yield_and_notify";
            if (cb.SelectedIndex() == 1) chosen = L"native_noop";
            else if (cb.SelectedIndex() == 2) chosen = L"override_always";

            auto taskbar = GetOrCreateSection(L"taskbar");
            taskbar.SetNamedValue(L"conflict_policy", JsonValue::CreateStringValue(chosen));
            SetSection(L"taskbar", taskbar);
            SaveConfiguration();
        });
        panel.Children().Append(conflictBox);

        // Opacity slider for maximized state
        double curOpacity = 0.85;
        if (taskbar.HasKey(L"states"))
        {
            try
            {
                auto states = taskbar.GetNamedObject(L"states");
                if (states.HasKey(L"maximized"))
                {
                    curOpacity = states.GetNamedObject(L"maximized").GetNamedNumber(L"opacity", 0.85);
                }
            }
            catch (...) {}
        }

        Slider blurSlider;
        blurSlider.Header(box_value(L"Maximized Taskbar Tint Opacity (%)"));
        blurSlider.Minimum(0);
        blurSlider.Maximum(100);
        blurSlider.Value(curOpacity * 100.0);
        blurSlider.ValueChanged([this](auto const& sender, auto const&) {
            if (m_isLoading) return;
            auto s = sender.template as<Slider>();
            auto taskbar = GetOrCreateSection(L"taskbar");
            JsonObject states;
            if (taskbar.HasKey(L"states"))
            {
                try { states = taskbar.GetNamedObject(L"states"); } catch (...) {}
            }
            JsonObject maximized;
            if (states.HasKey(L"maximized"))
            {
                try { maximized = states.GetNamedObject(L"maximized"); } catch (...) {}
            }
            maximized.SetNamedValue(L"opacity", JsonValue::CreateNumberValue(s.Value() / 100.0));
            states.SetNamedValue(L"maximized", maximized);
            taskbar.SetNamedValue(L"states", states);
            SetSection(L"taskbar", taskbar);
            SaveConfiguration();
        });
        panel.Children().Append(blurSlider);

        return panel;
    }

    UIElement MainWindow::CreateDesktopView()
    {
        StackPanel panel;
        panel.Spacing(16);

        TextBlock title;
        title.Text(L"Desktop Layering & Interactive Mode");
        title.FontSize(24);
        panel.Children().Append(title);

        TextBlock desc;
        desc.Text(L"Per PRD Q31:A: Desktop widgets are layered strictly below desktop icons and above wallpapers. Temporary Interactive Mode brings widgets into focus for manipulation without desktop icon obstruction.");
        desc.TextWrapping(TextWrapping::Wrap);
        panel.Children().Append(desc);

        auto desktop = GetOrCreateSection(L"desktop");

        ComboBox placementBox;
        placementBox.Header(box_value(L"Desktop Layering Stacking Mode (PRD Section 5.1)"));
        ComboBoxItem plBetween, plBehind, plTop;
        plBetween.Content(box_value(L"Between Wallpaper & Icons (Default)"));
        plBehind.Content(box_value(L"Behind Wallpaper"));
        plTop.Content(box_value(L"Above Desktop Icons (Temporary Overlay)"));
        placementBox.Items().Append(plBetween);
        placementBox.Items().Append(plBehind);
        placementBox.Items().Append(plTop);

        hstring curPl = desktop.GetNamedString(L"placement", L"between_wallpaper_and_icons");
        if (curPl == L"behind_wallpaper") placementBox.SelectedIndex(1);
        else if (curPl == L"always_on_top") placementBox.SelectedIndex(2);
        else placementBox.SelectedIndex(0);

        placementBox.SelectionChanged([this](IInspectable const& sender, SelectionChangedEventArgs const&) {
            if (m_isLoading) return;
            auto cb = sender.as<ComboBox>();
            hstring chosen = L"between_wallpaper_and_icons";
            if (cb.SelectedIndex() == 1) chosen = L"behind_wallpaper";
            else if (cb.SelectedIndex() == 2) chosen = L"always_on_top";

            auto desktop = GetOrCreateSection(L"desktop");
            desktop.SetNamedValue(L"placement", JsonValue::CreateStringValue(chosen));
            SetSection(L"desktop", desktop);
            SaveConfiguration();
        });
        panel.Children().Append(placementBox);

        Slider timeoutSlider;
        timeoutSlider.Header(box_value(L"Interactive Inactivity Timeout (Seconds)"));
        timeoutSlider.Minimum(1);
        timeoutSlider.Maximum(60);
        timeoutSlider.Value(desktop.GetNamedNumber(L"interactive_timeout_seconds", 8));
        timeoutSlider.ValueChanged([this](auto const& sender, auto const&) {
            if (m_isLoading) return;
            auto s = sender.template as<Slider>();
            auto desktop = GetOrCreateSection(L"desktop");
            desktop.SetNamedValue(L"interactive_timeout_seconds", JsonValue::CreateNumberValue(s.Value()));
            SetSection(L"desktop", desktop);
            SaveConfiguration();
        });
        panel.Children().Append(timeoutSlider);

        Slider snapGridSlider;
        snapGridSlider.Header(box_value(L"Widget Spatial Snap Grid (DIP)"));
        snapGridSlider.Minimum(2);
        snapGridSlider.Maximum(32);
        snapGridSlider.Value(desktop.GetNamedNumber(L"snap_grid_dip", 8.0));
        snapGridSlider.ValueChanged([this](auto const& sender, auto const&) {
            if (m_isLoading) return;
            auto s = sender.template as<Slider>();
            auto desktop = GetOrCreateSection(L"desktop");
            desktop.SetNamedValue(L"snap_grid_dip", JsonValue::CreateNumberValue(s.Value()));
            SetSection(L"desktop", desktop);
            SaveConfiguration();
        });
        panel.Children().Append(snapGridSlider);

        Button triggerBtn;
        triggerBtn.Content(box_value(L"Enter Temporary Interactive Mode (8s)"));
        triggerBtn.Click([](auto const&, auto const&) {
            ::AetherShell::Settings::SettingsIpcClient::SendCommand("enter_interactive_mode", "{\"timeout_seconds\":8}");
        });
        panel.Children().Append(triggerBtn);

        return panel;
    }

    UIElement MainWindow::CreateWidgetsView()
    {
        StackPanel panel;
        panel.Spacing(16);

        TextBlock title;
        title.Text(L"Widgets & Visual Themes");
        title.FontSize(24);
        panel.Children().Append(title);

        ComboBox profileBox;
        profileBox.Header(box_value(L"Profile Archetype"));
        const wchar_t* archetypes[] = {
            L"signature_minimal",
            L"cyber_hud",
            L"glass_bento",
            L"ambient_soft",
            L"terminal_mono",
            L"spectrum_dynamic"
        };
        for (const auto& arch : archetypes)
        {
            ComboBoxItem item;
            item.Content(box_value(arch));
            profileBox.Items().Append(item);
        }

        hstring curProfile = m_config ? m_config.GetNamedString(L"profile", L"signature_minimal") : L"signature_minimal";
        int selIdx = 0;
        for (int i = 0; i < 6; ++i)
        {
            if (curProfile == archetypes[i])
            {
                selIdx = i;
                break;
            }
        }
        profileBox.SelectedIndex(selIdx);

        profileBox.SelectionChanged([this, archetypes](IInspectable const& sender, SelectionChangedEventArgs const&) {
            if (m_isLoading) return;
            auto cb = sender.as<ComboBox>();
            int idx = cb.SelectedIndex();
            if (idx >= 0 && idx < 6)
            {
                m_config.SetNamedValue(L"profile", JsonValue::CreateStringValue(archetypes[idx]));
                SaveConfiguration();
                ::AetherShell::Settings::SettingsIpcClient::SendCommand("reload_profile");
            }
        });
        panel.Children().Append(profileBox);

        auto gaming = GetOrCreateSection(L"gaming");

        ToggleSwitch gamingSwitch;
        gamingSwitch.Header(box_value(L"Gaming Shield (Suspend Rendering During Fullscreen Games)"));
        gamingSwitch.IsOn(gaming.GetNamedBoolean(L"enabled", true));
        gamingSwitch.Toggled([this](IInspectable const& sender, RoutedEventArgs const&) {
            if (m_isLoading) return;
            auto s = sender.as<ToggleSwitch>();
            auto gaming = GetOrCreateSection(L"gaming");
            gaming.SetNamedValue(L"enabled", JsonValue::CreateBooleanValue(s.IsOn()));
            SetSection(L"gaming", gaming);
            SaveConfiguration();
        });
        panel.Children().Append(gamingSwitch);

        TextBlock info;
        info.Text(L"Enabled Built-in Widgets: System Clock, CPU/RAM/Battery Telemetry, WASAPI Spectrum Visualizer, Network Monitor.");
        panel.Children().Append(info);

        return panel;
    }

    UIElement MainWindow::CreateAudioView()
    {
        StackPanel panel;
        panel.Spacing(16);

        TextBlock title;
        title.Text(L"WASAPI Audio Visualization (PRD Section 10)");
        title.FontSize(24);
        panel.Children().Append(title);

        TextBlock privacy;
        privacy.Text(L"Strict Audio Privacy Guarantee (Gate G14): Only frequency magnitudes are processed for UI visualization. Raw PCM audio is never recorded, buffered to disk, or transmitted over network.");
        privacy.TextWrapping(TextWrapping::Wrap);
        panel.Children().Append(privacy);

        auto audio = GetOrCreateSection(L"audio");

        ToggleSwitch audioSwitch;
        audioSwitch.Header(box_value(L"WASAPI Shared-Loopback Audio Capture"));
        audioSwitch.IsOn(audio.GetNamedBoolean(L"visualizer_enabled", true));
        audioSwitch.Toggled([this](IInspectable const& sender, RoutedEventArgs const&) {
            if (m_isLoading) return;
            auto ts = sender.as<ToggleSwitch>();
            auto audio = GetOrCreateSection(L"audio");
            audio.SetNamedValue(L"visualizer_enabled", JsonValue::CreateBooleanValue(ts.IsOn()));
            SetSection(L"audio", audio);
            SaveConfiguration();

            if (ts.IsOn())
            {
                ::AetherShell::Settings::SettingsIpcClient::SendCommand("resume_audio");
            }
            else
            {
                ::AetherShell::Settings::SettingsIpcClient::SendCommand("pause_audio");
            }
        });
        panel.Children().Append(audioSwitch);

        ComboBox fpsBox;
        fpsBox.Header(box_value(L"Spectrum Visualization Render FPS"));
        ComboBoxItem fps15, fps30, fps60, fps120;
        fps15.Content(box_value(L"15 FPS (Battery Saver Default)"));
        fps30.Content(box_value(L"30 FPS (AC Balanced Default)"));
        fps60.Content(box_value(L"60 FPS (Smooth Performance)"));
        fps120.Content(box_value(L"120 FPS (High Refresh Rate)"));
        fpsBox.Items().Append(fps15);
        fpsBox.Items().Append(fps30);
        fpsBox.Items().Append(fps60);
        fpsBox.Items().Append(fps120);

        double curFps = audio.GetNamedNumber(L"render_fps", 30);
        if (curFps <= 15) fpsBox.SelectedIndex(0);
        else if (curFps <= 30) fpsBox.SelectedIndex(1);
        else if (curFps <= 60) fpsBox.SelectedIndex(2);
        else fpsBox.SelectedIndex(3);

        fpsBox.SelectionChanged([this](IInspectable const& sender, SelectionChangedEventArgs const&) {
            if (m_isLoading) return;
            auto cb = sender.as<ComboBox>();
            double chosenFps = 30;
            if (cb.SelectedIndex() == 0) chosenFps = 15;
            else if (cb.SelectedIndex() == 1) chosenFps = 30;
            else if (cb.SelectedIndex() == 2) chosenFps = 60;
            else if (cb.SelectedIndex() == 3) chosenFps = 120;

            auto audio = GetOrCreateSection(L"audio");
            audio.SetNamedValue(L"render_fps", JsonValue::CreateNumberValue(chosenFps));
            SetSection(L"audio", audio);
            SaveConfiguration();
        });
        panel.Children().Append(fpsBox);

        ComboBox bandsBox;
        bandsBox.Header(box_value(L"FFT Spectrum Frequency Bands"));
        ComboBoxItem b16, b32, b64, b128;
        b16.Content(box_value(L"16 Bands"));
        b32.Content(box_value(L"32 Bands"));
        b64.Content(box_value(L"64 Bands (Default)"));
        b128.Content(box_value(L"128 Bands (High Detail)"));
        bandsBox.Items().Append(b16);
        bandsBox.Items().Append(b32);
        bandsBox.Items().Append(b64);
        bandsBox.Items().Append(b128);

        double curBands = audio.GetNamedNumber(L"spectrum_bands", 64);
        if (curBands <= 16) bandsBox.SelectedIndex(0);
        else if (curBands <= 32) bandsBox.SelectedIndex(1);
        else if (curBands <= 64) bandsBox.SelectedIndex(2);
        else bandsBox.SelectedIndex(3);

        bandsBox.SelectionChanged([this](IInspectable const& sender, SelectionChangedEventArgs const&) {
            if (m_isLoading) return;
            auto cb = sender.as<ComboBox>();
            double chosenBands = 64;
            if (cb.SelectedIndex() == 0) chosenBands = 16;
            else if (cb.SelectedIndex() == 1) chosenBands = 32;
            else if (cb.SelectedIndex() == 2) chosenBands = 64;
            else if (cb.SelectedIndex() == 3) chosenBands = 128;

            auto audio = GetOrCreateSection(L"audio");
            audio.SetNamedValue(L"spectrum_bands", JsonValue::CreateNumberValue(chosenBands));
            SetSection(L"audio", audio);
            SaveConfiguration();
        });
        panel.Children().Append(bandsBox);

        Button querySpectrumBtn;
        querySpectrumBtn.Content(box_value(L"Query Active Audio Spectrum"));
        querySpectrumBtn.Click([](auto const&, auto const&) {
            ::AetherShell::Settings::SettingsIpcClient::SendCommand("get_spectrum");
        });
        panel.Children().Append(querySpectrumBtn);

        return panel;
    }

    UIElement MainWindow::CreateScriptsView()
    {
        StackPanel panel;
        panel.Spacing(16);

        TextBlock title;
        title.Text(L"Script Host & Security Isolation (PRD Section 11 / Q33:A)");
        title.FontSize(24);
        panel.Children().Append(title);

        TextBlock desc;
        desc.Text(L"Lua scripts execute inside an isolated supervisor process (aethershell-script-host.exe). Memory, file access, and execution time are strictly bounded.");
        desc.TextWrapping(TextWrapping::Wrap);
        panel.Children().Append(desc);

        auto scripting = GetOrCreateSection(L"scripting");

        ToggleSwitch scriptSwitch;
        scriptSwitch.Header(box_value(L"Enable Optional Lua Script Worker"));
        scriptSwitch.IsOn(scripting.GetNamedBoolean(L"enabled", true));
        scriptSwitch.Toggled([this](IInspectable const& sender, RoutedEventArgs const&) {
            if (m_isLoading) return;
            auto s = sender.as<ToggleSwitch>();
            auto scripting = GetOrCreateSection(L"scripting");
            scripting.SetNamedValue(L"enabled", JsonValue::CreateBooleanValue(s.IsOn()));
            SetSection(L"scripting", scripting);
            SaveConfiguration();
        });
        panel.Children().Append(scriptSwitch);

        ToggleSwitch trustSwitch;
        trustSwitch.Header(box_value(L"Require Explicit SHA-256 Digest Trust for Imported Scripts"));
        trustSwitch.IsOn(scripting.GetNamedBoolean(L"require_explicit_trust_for_imported_scripts", true));
        trustSwitch.Toggled([this](IInspectable const& sender, RoutedEventArgs const&) {
            if (m_isLoading) return;
            auto s = sender.as<ToggleSwitch>();
            auto scripting = GetOrCreateSection(L"scripting");
            scripting.SetNamedValue(L"require_explicit_trust_for_imported_scripts", JsonValue::CreateBooleanValue(s.IsOn()));
            SetSection(L"scripting", scripting);
            SaveConfiguration();
        });
        panel.Children().Append(trustSwitch);

        ComboBox crashLimitBox;
        crashLimitBox.Header(box_value(L"Worker Crash Quarantine Circuit Breaker Limit"));
        ComboBoxItem cr0, cr1, cr2, cr5;
        cr0.Content(box_value(L"0 (Quarantine on First Fault)"));
        cr1.Content(box_value(L"1 Restart"));
        cr2.Content(box_value(L"2 Restarts (Default)"));
        cr5.Content(box_value(L"5 Restarts"));
        crashLimitBox.Items().Append(cr0);
        crashLimitBox.Items().Append(cr1);
        crashLimitBox.Items().Append(cr2);
        crashLimitBox.Items().Append(cr5);

        double curLimit = scripting.GetNamedNumber(L"restart_after_crash_limit", 2);
        if (curLimit <= 0) crashLimitBox.SelectedIndex(0);
        else if (curLimit <= 1) crashLimitBox.SelectedIndex(1);
        else if (curLimit <= 2) crashLimitBox.SelectedIndex(2);
        else crashLimitBox.SelectedIndex(3);

        crashLimitBox.SelectionChanged([this](IInspectable const& sender, SelectionChangedEventArgs const&) {
            if (m_isLoading) return;
            auto cb = sender.as<ComboBox>();
            double chosenLimit = 2;
            if (cb.SelectedIndex() == 0) chosenLimit = 0;
            else if (cb.SelectedIndex() == 1) chosenLimit = 1;
            else if (cb.SelectedIndex() == 2) chosenLimit = 2;
            else if (cb.SelectedIndex() == 3) chosenLimit = 5;

            auto scripting = GetOrCreateSection(L"scripting");
            scripting.SetNamedValue(L"restart_after_crash_limit", JsonValue::CreateNumberValue(chosenLimit));
            SetSection(L"scripting", scripting);
            SaveConfiguration();
        });
        panel.Children().Append(crashLimitBox);

        return panel;
    }
}
