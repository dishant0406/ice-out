# Settings for dmgbuild: the look of the installer window. See scripts/package.sh.
# The picture (assets/dmg/background.html) is 660 x 400 points; the window is 32 points taller
# for its title bar. The app and the Applications shortcut sit on its left and right, with the
# arrow between them drawn into the picture.
import os

app = defines["app"]  # path to Iceout.app
background = defines["background"]  # the picture behind the icons
icon = defines["icon"]  # the icon of the mounted disk

format = "UDZO"
files = [app]
symlinks = {"Applications": "/Applications"}
window_rect = ((200, 140), (660, 432))
default_view = "icon-view"
show_status_bar = False
show_tab_view = False
show_toolbar = False
show_pathbar = False
show_sidebar = False
arrange_by = None
icon_size = 112
text_size = 13
icon_locations = {os.path.basename(app): (180, 190), "Applications": (480, 190)}
