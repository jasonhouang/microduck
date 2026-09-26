// SPDX-License-Identifier: GPL-2.0
/*
 * INMP441 I2S MEMS Microphone - Dummy ASoC Codec Driver
 *
 * This is a minimal codec driver for the INMP441 digital microphone.
 * The INMP441 is a pure I2S output device with no configuration registers -
 * it just outputs audio data on the I2S bus. This driver exists to satisfy
 * the ASoC framework's requirement for a codec component.
 *
 * Hardware: INMP441 outputs data on SD pin, clocked by BCLK and LRCK from host.
 * No I2C/SPI configuration needed - it's a data source only.
 *
 * Author: Microduck Project
 */

#include <linux/module.h>
#include <linux/platform_device.h>
#include <sound/soc.h>

/*
 * DAI (Digital Audio Interface) operations
 * The INMP441 only supports capture (recording), not playback.
 */
static const struct snd_soc_dai_ops inmp441_dai_ops = {
	/* No operations needed - INMP441 has no configuration */
};

static struct snd_soc_dai_driver inmp441_dai = {
	.name = "inmp441-hifi",
	.capture = {
		.stream_name = "Capture",
		.channels_min = 1,
		.channels_max = 2,  /* Support mono and stereo */
		.rates = SNDRV_PCM_RATE_8000_48000,
		.formats = SNDRV_PCM_FMTBIT_S16_LE | SNDRV_PCM_FMTBIT_S24_LE | SNDRV_PCM_FMTBIT_S32_LE,
	},
};

/*
 * Component driver - minimal implementation
 * INMP441 has no registers, no controls, no DAPM widgets
 */
static struct snd_soc_component_driver inmp441_component = {
	.name = "inmp441-codec",
};

static int inmp441_probe(struct platform_device *pdev)
{
	return snd_soc_register_component(&pdev->dev, &inmp441_component,
					  &inmp441_dai, 1);
}

static int inmp441_remove(struct platform_device *pdev)
{
	snd_soc_unregister_component(&pdev->dev);
	return 0;
}

static const struct of_device_id inmp441_of_match[] = {
	{ .compatible = "mems,inmp441" },
	{ }
};
MODULE_DEVICE_TABLE(of, inmp441_of_match);

static struct platform_driver inmp441_driver = {
	.driver = {
		.name = "inmp441",
		.of_match_table = inmp441_of_match,
	},
	.probe = inmp441_probe,
	.remove = inmp441_remove,
};

module_platform_driver(inmp441_driver);

MODULE_LICENSE("GPL");
MODULE_AUTHOR("Microduck Project");
MODULE_DESCRIPTION("Dummy ASoC codec driver for INMP441 I2S MEMS microphone");
MODULE_ALIAS("platform:inmp441");
