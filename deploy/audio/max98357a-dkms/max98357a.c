/*
 * MAX98357A I2S Amplifier - Dummy ASoC Codec Driver
 * 
 * MAX98357A is a simple I2S input Class-D amplifier with no configuration
 * interface. It just needs I2S data (BCLK, LRCK, DIN) and outputs to speaker.
 * 
 * This driver provides the ASoC codec interface for the device tree.
 */

#include <linux/module.h>
#include <linux/platform_device.h>
#include <sound/soc.h>

static struct snd_soc_dai_driver max98357a_dai = {
	.name = "max98357a-hifi",
	.playback = {
		.stream_name = "Playback",
		.channels_min = 1,
		.channels_max = 2,
		.rates = SNDRV_PCM_RATE_8000_48000,
		.formats = SNDRV_PCM_FMTBIT_S16_LE | SNDRV_PCM_FMTBIT_S24_LE | SNDRV_PCM_FMTBIT_S32_LE,
	},
};

static struct snd_soc_component_driver max98357a_component = {
	.name = "max98357a-codec",
};

static int max98357a_probe(struct platform_device *pdev)
{
	return snd_soc_register_component(&pdev->dev, &max98357a_component,
					  &max98357a_dai, 1);
}

static int max98357a_remove(struct platform_device *pdev)
{
	snd_soc_unregister_component(&pdev->dev);
	return 0;
}

static const struct of_device_id max98357a_of_match[] = {
	{ .compatible = "maxim,max98357a" },
	{ }
};
MODULE_DEVICE_TABLE(of, max98357a_of_match);

static struct platform_driver max98357a_driver = {
	.driver = {
		.name = "max98357a",
		.of_match_table = max98357a_of_match,
	},
	.probe = max98357a_probe,
	.remove = max98357a_remove,
};

module_platform_driver(max98357a_driver);

MODULE_LICENSE("GPL");
MODULE_AUTHOR("Microduck Project");
MODULE_DESCRIPTION("Dummy ASoC codec driver for MAX98357A I2S amplifier");
MODULE_ALIAS("platform:max98357a");
