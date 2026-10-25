#include "pch.h"
#include "MainWindow.xaml.h"
#if __has_include("MainWindow.g.cpp")
#include "MainWindow.g.cpp"
#endif
#include "IpcClient.h"

using namespace winrt;
using namespace Microsoft::UI::Xaml;
using namespace Microsoft::UI::Xaml::Controls;

namespace winrt::AetherShell::Settings::implementation
{
    MainWindow::MainWindow()
    {
        InitializeComponent();
        this->Closed({ this, &MainWindow::OnWindowClosed });
    }

    void MainWindow::NavView_SelectionChanged(
        NavigationView const&,
        NavigationViewSelectionChangedEventArgs const& args)
    {
        if (auto selectedItem = args.SelectedItemContainer())
        {
            auto tag = unbox_value<hstring>(selectedItem.Tag());
            // Switch views / navigate frame
        }
    }

    void MainWindow::OnWindowClosed(
        Windows::Foundation::IInspectable const&,
        WindowEventArgs const&)
    {
        // Enforce Gate G11: Process exits immediately on close to eliminate resident RAM footprint
        ExitProcess(0);
    }
}
